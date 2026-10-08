//! Read bounded, simple page layout for editable SpreadsheetML. A private COS copy
//! normalizes cross-stream text state; the source document is never rewritten.

use crate::{EditError, TextLine, text_lines};
use printcraft_content::{Matrix, parse};
use printcraft_cos::{Dict, Document, Object, Stream};
use std::collections::BTreeMap;

pub struct Text {
    pub line: TextLine,
    /// Display coordinates, top-left origin, points.
    pub rect: [f64; 4],
    pub size: f64,
}

#[derive(Clone, Copy)]
pub struct Rule {
    pub points: [f64; 4],
    pub color: [f64; 3],
    pub width: f64,
}

#[derive(Clone, Copy)]
pub struct Fill {
    pub rect: [f64; 4],
    pub color: [f64; 3],
}

#[derive(Default)]
pub struct Layout {
    pub text: Vec<Text>,
    pub rules: Vec<Rule>,
    pub fills: Vec<Fill>,
    /// Original top-level images in drawing order, in display coordinates.
    pub images: Vec<[f64; 4]>,
    /// Visible crops of special/clipped text; never copy its hidden character data.
    pub raster_text: Vec<[f64; 4]>,
    /// Complex visible content must use a whole-page image rather than disappear.
    pub complex: bool,
}

#[derive(Clone, Copy)]
struct State {
    matrix: Matrix,
    fill: [f64; 3],
    stroke: [f64; 3],
    width: f64,
    render: f64,
    opaque: bool,
    clip: Option<[f64; 4]>,
}

fn contains(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[0] + 0.1 && a[1] <= b[1] + 0.1 && a[2] >= b[2] - 0.1 && a[3] >= b[3] - 0.1
}
fn intersects(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] < b[2] - 0.1 && a[2] > b[0] + 0.1 && a[1] < b[3] - 0.1 && a[3] > b[1] + 0.1
}

pub fn layout(doc: &mut Document, page_index: usize) -> Result<Layout, EditError> {
    let page = printcraft_model::pages(doc).into_iter().nth(page_index).ok_or(EditError::NoSuchPage(page_index))?;
    let view = Matrix(page.view_matrix(doc)).invert().ok_or_else(|| EditError::Invalid("页面坐标无效".into()))?;
    let (width, height) = page.display_size(doc);
    let bounds = [0.0, 0.0, width, height];
    let entries = match page.dict.get(b"Contents") {
        Some(o) => match &*doc.resolve(o) {
            Object::Array(a) => a.clone(),
            _ => vec![o.clone()],
        },
        None => Vec::new(),
    };
    if entries.len() > 512 {
        return Err(EditError::Invalid("页面内容流过多".into()));
    }
    let mut data = Vec::new();
    for entry in entries {
        let Object::Stream(s) = &*doc.resolve(&entry) else {
            return Err(EditError::Invalid("页面内容流无效".into()));
        };
        let bytes = s.decoded()?;
        if data.len().saturating_add(bytes.len()).saturating_add(1) > 16 * 1024 * 1024 {
            return Err(EditError::Invalid("页面内容过大".into()));
        }
        data.extend(bytes);
        data.push(b'\n');
    }
    let parsed = parse(&data);
    if parsed.skipped != 0 || parsed.ops.len() > 200_000 {
        return Err(EditError::Invalid("页面内容无法完整读取".into()));
    }
    let r = doc.add(Object::Stream(Stream::from_raw(Dict::new(), data)));
    doc.update_dict(page.obj, |d| d.set(b"Contents".to_vec(), Object::Ref(r)))?;
    let resources = page.dict.get(b"Resources").and_then(|o| doc.resolve(o).as_dict().cloned()).unwrap_or_default();
    let gs = resources.get(b"ExtGState").and_then(|o| doc.resolve(o).as_dict().cloned()).unwrap_or_default();
    let objects = resources.get(b"XObject").and_then(|o| doc.resolve(o).as_dict().cloned()).unwrap_or_default();
    let point = |m: Matrix, x: f64, y: f64| {
        let (x, y) = m.apply(x, y);
        let (x, y) = view.apply(x, y);
        [x, height - y]
    };
    let annotations=page.dict.get(b"Annots").is_some_and(|a|matches!(&*doc.resolve(a),Object::Array(v) if v.iter().any(|a| {
        let o=doc.resolve(a);let Some(d)=o.as_dict() else {return true;};
        // An invisible link adds no artwork; do not rasterize a whole contents page.
        !(d.name(b"Subtype")==Some(b"Link") && d.get(b"AP").is_none() && d.get(b"BS").is_some_and(|b|doc.resolve(b).as_dict().and_then(|b|b.get(b"W")).and_then(Object::as_f64)==Some(0.0)))
    })));
    let unit = page.dict.get(b"UserUnit").and_then(Object::as_f64).unwrap_or(1.0);
    let mut out = Layout { complex: annotations || !unit.is_finite() || (unit - 1.0).abs() > 1e-6, ..Layout::default() };
    let mut state = State { matrix: Matrix::IDENTITY, fill: [0.0; 3], stroke: [0.0; 3], width: 1.0, render: 0.0, opaque: true, clip: Some(bounds) };
    let (mut stack, mut path, mut rectangles) = (Vec::new(), Vec::new(), Vec::new());
    let (mut current, mut start) = (None, None);
    let mut shown = BTreeMap::new();
    let mut painted = Vec::new();
    let mut image_paints = Vec::new();
    for (i, op) in parsed.ops.iter().enumerate() {
        match op.op.as_slice() {
            b"q" => {
                if stack.len() >= 1024 {
                    return Err(EditError::Invalid("页面图形状态过深".into()));
                }
                stack.push(state);
            }
            b"Q" => state = stack.pop().ok_or_else(|| EditError::Invalid("页面图形状态不完整".into()))?,
            b"cm" => {
                if let Some(m) = op.nums::<6>() {
                    state.matrix = Matrix(m).then(&state.matrix);
                }
            }
            b"w" => state.width = op.num(0).unwrap_or(1.0),
            b"Tr" => state.render = op.num(0).unwrap_or(-1.0),
            b"g" | b"G" => {
                if let Some(v) = op.num(0) {
                    if op.is("g") {
                        state.fill = [v; 3];
                    } else {
                        state.stroke = [v; 3];
                    }
                }
            }
            b"rg" | b"RG" => {
                if let Some(v) = op.nums::<3>() {
                    if op.is("rg") {
                        state.fill = v;
                    } else {
                        state.stroke = v;
                    }
                }
            }
            b"k" | b"K" => {
                if let Some([c, m, y, k]) = op.nums::<4>() {
                    let v = [(1.0 - c) * (1.0 - k), (1.0 - m) * (1.0 - k), (1.0 - y) * (1.0 - k)];
                    if op.is("k") {
                        state.fill = v;
                    } else {
                        state.stroke = v;
                    }
                }
            }
            b"gs" => {
                let d = op.name(0).and_then(|n| gs.get(n)).and_then(|o| doc.resolve(o).as_dict().cloned());
                state.opaque &= d.is_some_and(|d| {
                    d.get(b"ca").and_then(Object::as_f64).is_none_or(|v| (v - 1.0).abs() < 1e-6)
                        && d.get(b"CA").and_then(Object::as_f64).is_none_or(|v| (v - 1.0).abs() < 1e-6)
                        && d.get(b"BM").is_none_or(|o| o.as_name() == Some(b"Normal"))
                        && d.get(b"SMask").is_none_or(|o| o.as_name() == Some(b"None"))
                });
            }
            b"m" => {
                if let Some([x, y]) = op.nums::<2>() {
                    current = Some(point(state.matrix, x, y));
                    start = current;
                }
            }
            b"l" => {
                if let (Some(a), Some([x, y])) = (current, op.nums::<2>()) {
                    let b = point(state.matrix, x, y);
                    path.push([a[0], a[1], b[0], b[1]]);
                    current = Some(b);
                }
            }
            b"h" | b"s" | b"b" | b"b*" => {
                if let (Some(a), Some(b)) = (current, start) {
                    path.push([a[0], a[1], b[0], b[1]]);
                }
            }
            b"re" => {
                if let Some([x, y, w, h]) = op.nums::<4>() {
                    let pts =
                        [point(state.matrix, x, y), point(state.matrix, x + w, y), point(state.matrix, x + w, y + h), point(state.matrix, x, y + h)];
                    for j in 0..4 {
                        if let (Some(a), Some(b)) = (pts.get(j), pts.get((j + 1) % 4)) {
                            path.push([a[0], a[1], b[0], b[1]]);
                        }
                    }
                    let rect = [
                        pts.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min),
                        pts.iter().map(|p| p[1]).fold(f64::INFINITY, f64::min),
                        pts.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max),
                        pts.iter().map(|p| p[1]).fold(f64::NEG_INFINITY, f64::max),
                    ];
                    rectangles.push(rect);
                }
            }
            b"W" | b"W*" => {
                state.clip = if rectangles.len() == 1 && path.len() == 4 {
                    rectangles.first().zip(state.clip).map(|(r, c)| [r[0].max(c[0]), r[1].max(c[1]), r[2].min(c[2]), r[3].min(c[3])])
                } else {
                    out.complex = true;
                    None
                };
            }
            b"Tj" | b"TJ" | b"'" | b"\"" => {
                shown.insert(i, state);
            }
            b"Do" => {
                let image = op
                    .name(0)
                    .and_then(|n| objects.get(n))
                    .is_some_and(|o| matches!(&*doc.resolve(o), Object::Stream(s) if s.dict.name(b"Subtype")==Some(b"Image")));
                let m = state.matrix.then(&view);
                let r = m.bbox([0.0, 0.0, 1.0, 1.0]);
                let rect = [r[0], height - r[3], r[2], height - r[1]];
                if !image
                    || !state.opaque
                    || m.0[0] <= 0.0
                    || m.0[3] <= 0.0
                    || m.0[1].abs() > 0.001
                    || m.0[2].abs() > 0.001
                    || !state.clip.is_some_and(|c| contains(c, rect))
                {
                    out.complex = true;
                }
                out.images.push(rect);
                painted.push((i, rect));
                image_paints.push((i, rect));
            }
            b"BDC" | b"DP" => out.complex = true,
            b"cs" | b"CS" | b"sc" | b"SC" | b"scn" | b"SCN" | b"sh" | b"BI" | b"c" | b"v" | b"y" => out.complex = true,
            _ => {}
        }
        if path.len() > 5000 || rectangles.len() > 2000 || shown.len() > 2000 || painted.len() > 5000 {
            return Err(EditError::Invalid("页面对象过多，请使用原样保真".into()));
        }
        let stroke = matches!(op.op.as_slice(), b"S" | b"s" | b"B" | b"B*" | b"b" | b"b*");
        let fill = matches!(op.op.as_slice(), b"f" | b"f*" | b"F" | b"B" | b"B*" | b"b" | b"b*");
        if stroke || fill {
            if !state.opaque {
                out.complex = true;
            }
            if stroke {
                for &p in &path {
                    if (p[0] - p[2]).abs() < 0.1 || (p[1] - p[3]).abs() < 0.1 {
                        if let Some(c) = state.clip {
                            if c[2] <= c[0] || c[3] <= c[1] {
                                continue;
                            }
                            let mut points = p;
                            if (p[0] - p[2]).abs() < 0.1 {
                                if p[0] < c[0] || p[0] > c[2] {
                                    continue;
                                }
                                points[1] = p[1].clamp(c[1], c[3]);
                                points[3] = p[3].clamp(c[1], c[3]);
                            } else {
                                if p[1] < c[1] || p[1] > c[3] {
                                    continue;
                                }
                                points[0] = p[0].clamp(c[0], c[2]);
                                points[2] = p[2].clamp(c[0], c[2]);
                            }
                            let m = state.matrix.then(&view).0;
                            let scale = ((m[0].hypot(m[1]) + m[2].hypot(m[3])) / 2.0).abs();
                            out.rules.push(Rule { points, color: state.stroke, width: state.width * scale });
                            let half = state.width * scale / 2.0;
                            painted.push((
                                i,
                                [
                                    points[0].min(points[2]) - half,
                                    points[1].min(points[3]) - half,
                                    points[0].max(points[2]) + half,
                                    points[1].max(points[3]) + half,
                                ],
                            ));
                        } else {
                            out.complex = true;
                        }
                    } else {
                        out.complex = true;
                    }
                }
            }
            if fill {
                if rectangles.is_empty() && !path.is_empty() {
                    out.complex = true;
                }
                for &r in &rectangles {
                    let Some(c) = state.clip else {
                        out.complex = true;
                        continue;
                    };
                    let r = [r[0].max(c[0]), r[1].max(c[1]), r[2].min(c[2]), r[3].min(c[3])];
                    if r[2] <= r[0] || r[3] <= r[1] {
                        continue;
                    }
                    painted.push((i, r));
                    if (r[2] - r[0]).min(r[3] - r[1]) < 1.0 {
                        let points = if r[2] - r[0] > r[3] - r[1] {
                            [r[0], (r[1] + r[3]) / 2.0, r[2], (r[1] + r[3]) / 2.0]
                        } else {
                            [(r[0] + r[2]) / 2.0, r[1], (r[0] + r[2]) / 2.0, r[3]]
                        };
                        out.rules.push(Rule { points, color: state.fill, width: (r[2] - r[0]).min(r[3] - r[1]) });
                    } else {
                        out.fills.push(Fill { rect: r, color: state.fill });
                    }
                }
            }
        }
        if stroke || fill || op.is("n") {
            path.clear();
            rectangles.clear();
            current = None;
            start = None;
        }
    }
    // A covered original bitmap must not be embedded with its hidden pixels intact.
    if image_paints.iter().any(|(i, r)| painted.iter().any(|(later, p)| later > i && intersects(*r, *p))) {
        out.complex = true;
    }
    if out.rules.len() > 5000 || out.fills.len() > 5000 {
        return Err(EditError::Invalid("页面线条过多".into()));
    }
    let lines = text_lines(doc, page_index)?;
    if lines.len() > 2000 {
        return Err(EditError::Invalid("页面文字过多".into()));
    }
    for line in lines {
        let (_, indices, _, matrix) = line.source_ops();
        if indices.iter().all(|i| shown.get(i).is_some_and(|s| s.render == 3.0)) {
            continue;
        }
        let r = view.bbox(line.rect);
        let rect = [r[0], height - r[3], r[2], height - r[1]];
        let m = Matrix(matrix).then(&view).0;
        let font = line.base_font.to_ascii_lowercase();
        let safe = line.decodable && !line.text.chars().any(char::is_control) && !font.contains("wingdings") && !font.contains("webdings") && !font.ends_with("symbol")
            && m.iter().all(|v|v.is_finite()) && m[0]>0.0 && m[3]>0.0 && m[1].abs()<0.001 && m[2].abs()<0.001
            && rect.iter().all(|v|v.is_finite()) && contains(bounds,rect)
            // Never reveal text concealed by a later opaque object (e.g. a redaction).
            && !painted.iter().any(|(i,r)|indices.iter().any(|shown|i>shown) && intersects(*r,rect))
            && indices.iter().all(|i|shown.get(i).is_some_and(|s|s.render==0.0 && s.opaque && s.clip.is_some_and(|c|contains(c,rect))));
        if !safe {
            if !rect.iter().all(|v| v.is_finite()) {
                out.complex = true;
                continue;
            }
            let mut crop = [(rect[0] - 0.5).max(0.0), (rect[1] - 0.5).max(0.0), (rect[2] + 0.5).min(width), (rect[3] + 0.5).min(height)];
            for index in indices {
                if let Some(clip) = shown.get(index).and_then(|s| s.clip) {
                    crop = [crop[0].max(clip[0]), crop[1].max(clip[1]), crop[2].min(clip[2]), crop[3].min(clip[3])];
                } else {
                    out.complex = true;
                }
            }
            if crop[2] > crop[0] && crop[3] > crop[1] {
                out.raster_text.push(crop);
            }
            continue;
        }
        let size = line.size * (r[3] - r[1]) / (line.rect[3] - line.rect[1]);
        if !size.is_finite() || size <= 0.0 {
            out.complex = true;
            continue;
        }
        out.text.push(Text { line, rect, size });
    }
    // Adjacent glyph bounds can overlap (notably mixed-font page numbers). Rasterize
    // that small neighbourhood as one crop instead of rasterizing the entire page.
    for crop in &mut out.raster_text {
        while let Some(index) = out.text.iter().position(|t| intersects(*crop, t.rect)) {
            let t = out.text.remove(index);
            let r = t.rect;
            *crop = [crop[0].min(r[0]).max(0.0), crop[1].min(r[1]).max(0.0), crop[2].max(r[2]).min(width), crop[3].max(r[3]).min(height)];
        }
    }
    if out.raster_text.len() > 256 {
        out.complex = true;
    }
    Ok(out)
}
