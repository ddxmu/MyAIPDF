//! Conservative watermark candidates, never automatic deletion. Preserve all unselected
//! source bytes and graphics state. Independent text objects, marked watermark artifacts,
//! and image/Form placements are selectable; a full-page scan is deliberately excluded.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use printcraft_content::{Matrix, Op, parse};
use printcraft_cos::{Dict, Document, Object, Stream};
use sha2::{Digest, Sha256};

use crate::EditError;

const MAX_PAGES: usize = 500;
const MAX_STREAM_BYTES: usize = 16 * 1024 * 1024;
const MAX_CANDIDATES: usize = 4000;

#[derive(Clone, Debug, PartialEq)]
pub struct Occurrence {
    pub page: usize,
    /// Display space, top-left origin, in points.
    pub rect: [f64; 4],
    stream: usize,
    ranges: Vec<Range<usize>>,
    fingerprint: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    /// Bound to the actual page/stream bytes; edits require a fresh analysis.
    pub id: String,
    pub kind: &'static str,
    pub label: String,
    pub reason: String,
    pub likely: bool,
    pub occurrences: Vec<Occurrence>,
}

fn digest(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}

fn streams(doc: &Document, page: &Dict) -> Result<Vec<Vec<u8>>, EditError> {
    let entries = match page.get(b"Contents") {
        None => Vec::new(),
        Some(o) => match &*doc.resolve(o) {
            Object::Array(a) => a.clone(),
            _ => vec![o.clone()],
        },
    };
    if entries.len() > 512 {
        return Err(EditError::Invalid("页面内容过多，请缩小分析范围".into()));
    }
    entries
        .iter()
        .map(|o| match &*doc.resolve(o) {
            Object::Stream(s) => {
                let bytes = s.decoded()?;
                if bytes.len() > MAX_STREAM_BYTES {
                    return Err(EditError::Invalid("页面内容流过大，请先优化文件".into()));
                }
                Ok(bytes)
            }
            _ => Err(EditError::Invalid("页面内容流无效，未改动文档".into())),
        })
        .collect()
}

fn paint(op: &Op) -> bool {
    matches!(op.op.as_slice(), b"Tj" | b"TJ" | b"'" | b"\"" | b"Do" | b"BI" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" | b"sh")
}

fn marked_watermark(doc: &Document, op: &Op, resources: &Dict) -> bool {
    if !op.is("BDC") || op.name(0) != Some(b"Artifact") {
        return false;
    }
    let props = match op.operands.get(1) {
        Some(Object::Name(n)) => resources
            .get(b"Properties")
            .map(|o| doc.resolve(o))
            .and_then(|o| o.as_dict().and_then(|d| d.get(n).cloned()))
            .map(|o| doc.resolve(&o).as_dict().cloned()),
        Some(o) => Some(doc.resolve(o).as_dict().cloned()),
        None => None,
    }
    .flatten();
    props.is_some_and(|d| d.name(b"Subtype") == Some(b"Watermark") || d.name(b"PCMark") == Some(b"Watermark"))
}

fn box_in_view(doc: &Document, page: &printcraft_model::Page, r: [f64; 4]) -> [f64; 4] {
    let b = Matrix(page.view_matrix(doc)).invert().map(|m| m.bbox(r)).unwrap_or(r);
    let h = page.display_size(doc).1;
    [b[0], h - b[3], b[2], h - b[1]]
}

fn add(
    groups: &mut BTreeMap<(String, String), Candidate>,
    kind: &'static str,
    label: String,
    reason: &str,
    likely: bool,
    occurrence: Occurrence,
) -> Result<(), EditError> {
    if groups.values().map(|c| c.occurrences.len()).sum::<usize>() >= MAX_CANDIDATES {
        return Err(EditError::Invalid("可分析对象过多，请按页分批分析".into()));
    }
    let key = (kind.to_owned(), label.clone());
    let c = groups.entry(key).or_insert_with(|| Candidate { id: String::new(), kind, label, reason: reason.into(), likely, occurrences: Vec::new() });
    c.likely |= likely;
    c.occurrences.push(occurrence);
    Ok(())
}

/// Return likely candidates, or all safely separable objects when `include_all` is true.
/// Repetition is only a hint: every candidate starts unselected in the UI.
pub fn analyze(doc: &Document, selected: &[usize], include_all: bool) -> Result<Vec<Candidate>, EditError> {
    if selected.is_empty() || selected.len() > MAX_PAGES {
        return Err(EditError::Invalid("请选择 1 至 500 页进行分析".into()));
    }
    let all = printcraft_model::pages(doc);
    let mut unique = BTreeSet::new();
    let mut groups = BTreeMap::new();
    for &pi in selected {
        if !unique.insert(pi) {
            return Err(EditError::Invalid("分析页码重复".into()));
        }
        let page = all.get(pi).ok_or(EditError::NoSuchPage(pi))?;
        let resources = page.dict.get(b"Resources").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
        let xo = resources.get(b"XObject").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
        let gs = resources.get(b"ExtGState").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
        let lines = crate::text_lines(doc, pi)?;
        let (pw, ph) = page.display_size(doc);
        let full = [0.0, 0.0, pw, ph];
        let mut ctm = Matrix::IDENTITY;
        let mut alpha = 1.0_f64;
        let mut states = Vec::new();
        for (si, data) in streams(doc, &page.dict)?.iter().enumerate() {
            let parsed = parse(data);
            // Never serialize a repaired stream and silently drop unknown source data.
            if parsed.skipped != 0 {
                continue;
            }
            let ops = &parsed.ops;
            let fp = digest(data);
            let mut covered = BTreeSet::new();
            let mut marked: Vec<(usize, bool)> = Vec::new();
            let mut alphas = Vec::with_capacity(ops.len());
            for (oi, op) in ops.iter().enumerate() {
                alphas.push(alpha);
                match op.op.as_slice() {
                    b"q" => {
                        if states.len() >= 128 {
                            return Err(EditError::Invalid("图形状态嵌套过深".into()));
                        }
                        states.push((ctm, alpha));
                    }
                    b"Q" => {
                        if let Some((m, a)) = states.pop() {
                            ctm = m;
                            alpha = a;
                        }
                    }
                    b"cm" => {
                        if let Some(m) = op.nums::<6>() {
                            ctm = Matrix(m).then(&ctm);
                        }
                    }
                    b"gs" => {
                        if let Some(d) = op.name(0).and_then(|n| gs.get(n)).map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()) {
                            alpha = d.get(b"ca").and_then(Object::as_f64).unwrap_or(alpha).clamp(0.0, 1.0);
                        }
                    }
                    b"BDC" | b"BMC" => {
                        if marked.len() >= 128 {
                            return Err(EditError::Invalid("标记内容嵌套过深".into()));
                        }
                        marked.push((oi, marked_watermark(doc, op, &resources)));
                    }
                    b"EMC" => {
                        if let Some((start, true)) = marked.pop() {
                            let indices: Vec<_> = (start..oi).filter(|&j| ops.get(j).is_some_and(paint)).collect();
                            // Text inside a mark that is only part of a BT is not safely separable:
                            // deleting it could move unselected following text on the same baseline.
                            let partial_text =
                                indices.iter().any(|&j| ops.get(j).is_some_and(|o| matches!(o.op.as_slice(), b"Tj" | b"TJ" | b"'" | b"\"")))
                                    && !ops.get(start..oi).is_some_and(|v| v.iter().any(|o| o.is("BT")) && v.iter().any(|o| o.is("ET")));
                            if !indices.is_empty() && !partial_text {
                                let ranges = indices.iter().filter_map(|&j| ops.get(j).map(|o| o.span.clone())).collect();
                                covered.extend(indices);
                                let marked_lines: Vec<_> = lines
                                    .iter()
                                    .filter(|l| {
                                        let (stream, shows, _, _) = l.source_ops();
                                        stream == si && shows.iter().all(|j| *j > start && *j < oi)
                                    })
                                    .collect();
                                let label_text = marked_lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join(" · ");
                                let rect = marked_lines
                                    .iter()
                                    .map(|l| box_in_view(doc, page, l.rect))
                                    .reduce(|a, b| [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])])
                                    .unwrap_or(full);
                                let label = if label_text.is_empty() {
                                    format!("已标记水印 · 图形 {}", &digest(data.get(ops[start].span.start..op.span.end).unwrap_or_default())[..12])
                                } else {
                                    format!("已标记水印 · {label_text}")
                                };
                                add(
                                    &mut groups,
                                    "artifact",
                                    label,
                                    "PDF 内容明确标记为 Watermark",
                                    true,
                                    Occurrence { page: pi, rect, stream: si, ranges, fingerprint: fp.clone() },
                                )?;
                            }
                        }
                    }
                    b"Do" => {
                        let Some(obj) = op.name(0).and_then(|n| xo.get(n)).map(|o| doc.resolve(o)) else {
                            continue;
                        };
                        let Object::Stream(s) = &*obj else {
                            continue;
                        };
                        let sub = s.dict.name(b"Subtype");
                        if !matches!(sub, Some(b"Image") | Some(b"Form")) {
                            continue;
                        }
                        let natural = if sub == Some(b"Image") {
                            [0.0, 0.0, 1.0, 1.0]
                        } else {
                            match s.dict.get(b"BBox").and_then(Object::as_array) {
                                Some(a) if a.len() == 4 => {
                                    let Some(r) = a.iter().map(Object::as_f64).collect::<Option<Vec<_>>>() else {
                                        continue;
                                    };
                                    [r[0], r[1], r[2], r[3]]
                                }
                                _ => continue,
                            }
                        };
                        let form_matrix = s
                            .dict
                            .get(b"Matrix")
                            .and_then(Object::as_array)
                            .and_then(|a| {
                                let v = a.iter().map(Object::as_f64).collect::<Option<Vec<_>>>()?;
                                <[f64; 6]>::try_from(v).ok()
                            })
                            .map(Matrix)
                            .unwrap_or(Matrix::IDENTITY);
                        let rect = box_in_view(doc, page, form_matrix.then(&ctm).bbox(natural));
                        let area = ((rect[2] - rect[0]).abs() * (rect[3] - rect[1]).abs()) / (pw * ph).max(1.0);
                        if !area.is_finite() || area >= 0.8 || area <= 0.0001 {
                            continue;
                        }
                        let kind = if sub == Some(b"Image") { "image" } else { "form" };
                        let label = format!("{} · {}", if kind == "image" { "独立图片" } else { "独立图形" }, &digest(&s.raw)[..12]);
                        add(
                            &mut groups,
                            kind,
                            label,
                            "独立对象；可能是水印，也可能是正文插图，请预览确认",
                            alpha < 0.65,
                            Occurrence { page: pi, rect, stream: si, ranges: vec![op.span.clone()], fingerprint: fp.clone() },
                        )?;
                    }
                    _ => {}
                }
            }
            // Only a whole, independent text object is selectable. Never delete a word
            // embedded in a body paragraph or disturb that paragraph's implicit advances.
            for line in lines.iter().filter(|l| l.source_ops().0 == si && l.decodable) {
                let (_, shows, bt, matrix) = line.source_ops();
                if shows.iter().any(|i| covered.contains(i)) {
                    continue;
                }
                let Some(end) = ops.iter().enumerate().skip(bt.saturating_add(1)).find(|(_, o)| o.is("ET")).map(|(i, _)| i) else {
                    continue;
                };
                let in_bt: Vec<_> = (bt..end).filter(|&j| ops.get(j).is_some_and(paint)).collect();
                if in_bt.as_slice() != shows || in_bt.is_empty() {
                    continue;
                }
                let low_alpha = shows.iter().any(|&i| alphas.get(i).is_some_and(|a| *a < 0.65));
                let rotated = matrix[1].abs() + matrix[2].abs() > 0.05;
                let lower = line.text.trim().to_lowercase();
                let keyword = ["draft", "confidential", "watermark", "sample", "水印", "机密", "草稿", "样本", "仅供", "内部资料"]
                    .iter()
                    .any(|k| lower.contains(k));
                let likely = low_alpha || rotated || (keyword && line.size >= 18.0);
                let reason = if low_alpha {
                    "半透明文字，疑似水印"
                } else if rotated {
                    "旋转文字，疑似水印"
                } else if line.size >= 24.0 {
                    "大号独立文字；请确认不是标题或正文"
                } else {
                    "独立文字对象；跨页重复或水印关键词仅作为提示"
                };
                add(
                    &mut groups,
                    "text",
                    line.text.clone(),
                    reason,
                    likely,
                    Occurrence {
                        page: pi,
                        rect: box_in_view(doc, page, line.rect),
                        stream: si,
                        ranges: shows.iter().filter_map(|&i| ops.get(i).map(|o| o.span.clone())).collect(),
                        fingerprint: fp.clone(),
                    },
                )?;
            }
            // Marked placements may have been tentatively offered before their EMC.
            for c in groups.values_mut().filter(|c| matches!(c.kind, "image" | "form")) {
                c.occurrences.retain(|o| {
                    o.page != pi || o.stream != si || !o.ranges.iter().any(|r| covered.iter().any(|&j| ops.get(j).is_some_and(|p| p.span == *r)))
                });
            }
        }
    }
    let mut result: Vec<_> = groups.into_values().filter(|c| !c.occurrences.is_empty()).collect();
    for c in &mut result {
        let pages: BTreeSet<_> = c.occurrences.iter().map(|o| o.page).collect();
        if pages.len() >= 2 && c.kind == "text" && c.reason.starts_with("大号独立文字") {
            c.likely = true;
            c.reason = "跨页重复的大号文字，请确认不是标题或页眉".into();
        }
        if pages.len() >= 2 && c.kind != "text" {
            c.likely = true;
            c.reason = "跨页重复的独立图片/图形，请确认不是页眉 Logo 或正文插图".into();
        }
        let mut hash = Sha256::new();
        hash.update(c.kind.as_bytes());
        hash.update(c.label.as_bytes());
        for o in &c.occurrences {
            hash.update(o.fingerprint.as_bytes());
            hash.update(o.page.to_le_bytes());
            hash.update(o.stream.to_le_bytes());
            for n in &o.rect {
                hash.update(n.to_bits().to_le_bytes());
            }
            for r in &o.ranges {
                hash.update(r.start.to_le_bytes());
                hash.update(r.end.to_le_bytes());
            }
        }
        c.id = format!("{:x}", hash.finalize());
    }
    result.retain(|c| include_all || c.likely);
    result.sort_by_key(|c| (!c.likely, c.kind != "artifact", c.label.clone()));
    Ok(result)
}

/// Apply explicitly selected, current candidates. The engine executes this on a clone,
/// making validation/failure atomic and the complete operation one undo step.
pub fn remove(doc: &mut Document, pages: &[usize], ids: &[String]) -> Result<usize, EditError> {
    if ids.is_empty() {
        return Err(EditError::Invalid("请先选择要删除的候选水印".into()));
    }
    let candidates = analyze(doc, pages, true)?;
    let wanted: BTreeSet<_> = ids.iter().collect();
    if wanted.len() != ids.len() || wanted.iter().any(|id| !candidates.iter().any(|c| &c.id == *id)) {
        return Err(EditError::Invalid("分析结果已经过期或选择无效，请重新分析水印；文档未改动".into()));
    }
    let mut edits: BTreeMap<(usize, usize), Vec<Range<usize>>> = BTreeMap::new();
    let mut count = 0;
    for c in candidates.iter().filter(|c| wanted.contains(&c.id)) {
        for o in &c.occurrences {
            edits.entry((o.page, o.stream)).or_default().extend(o.ranges.clone());
            count += 1;
        }
    }
    let all = printcraft_model::pages(doc);
    for ((pi, si), mut ranges) in edits {
        let page = all.get(pi).ok_or(EditError::NoSuchPage(pi))?;
        let data = streams(doc, &page.dict)?.into_iter().nth(si).ok_or_else(|| EditError::Invalid("内容流已经改变".into()))?;
        ranges.sort_by_key(|r| r.start);
        ranges.dedup();
        let mut bytes = Vec::with_capacity(data.len());
        let mut at = 0;
        for r in ranges {
            let kept = data.get(at..r.start).ok_or_else(|| EditError::Invalid("候选范围重叠，未改动文档".into()))?;
            bytes.extend_from_slice(kept);
            let removed = data.get(r.clone()).ok_or_else(|| EditError::Invalid("候选范围无效".into()))?;
            // n consumes an unfinished path, just as a stroke/fill does. Text and Do/BI
            // removal need only whitespace; all non-painting state operators remain.
            match parse(removed).ops.first() {
                Some(o) if matches!(o.op.as_slice(), b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*") => {
                    bytes.extend_from_slice(b" n ")
                }
                Some(o) if o.is("'") => bytes.extend_from_slice(b" T* "),
                Some(o) if o.is("\"") => {
                    let mut state = Vec::new();
                    if let Some(n) = o.operands.first() {
                        state.push(Op::new("Tw", vec![n.clone()]));
                    }
                    if let Some(n) = o.operands.get(1) {
                        state.push(Op::new("Tc", vec![n.clone()]));
                    }
                    state.push(Op::new("T*", Vec::new()));
                    bytes.extend_from_slice(&printcraft_content::serialize_ops(&state));
                }
                _ => bytes.push(b' '),
            }
            at = r.end;
        }
        bytes.extend_from_slice(data.get(at..).ok_or_else(|| EditError::Invalid("候选范围无效".into()))?);
        let mut entries = match doc.get(page.obj).as_dict().and_then(|d| d.get(b"Contents").cloned()) {
            Some(o) => match &*doc.resolve(&o) {
                Object::Array(a) => a.clone(),
                _ => vec![o],
            },
            None => return Err(EditError::Invalid("页面内容已改变".into())),
        };
        let slot = entries.get_mut(si).ok_or_else(|| EditError::Invalid("页面内容已改变".into()))?;
        let mut dictionary = match &*doc.resolve(slot) {
            Object::Stream(s) => s.dict.clone(),
            _ => Dict::new(),
        };
        dictionary.remove(b"Filter");
        dictionary.remove(b"DecodeParms");
        *slot = Object::Ref(doc.add(Object::Stream(Stream::flate(dictionary, &bytes))));
        doc.update_dict(page.obj, |d| d.set(b"Contents".to_vec(), Object::Array(entries)))?;
    }
    Ok(count)
}
