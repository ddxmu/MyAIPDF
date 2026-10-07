//! Bounded copy-on-write production edits. Grayscale covers explicit vector/text DeviceRGB
//! and DeviceCMYK operators, not images, ICC/spot colours or annotation appearances.

use crate::EditError;
use printcraft_content::{Op, num, parse, serialize_ops};
use printcraft_cos::{Dict, Document, ObjRef, Object, Stream};
use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq)]
pub enum Settings {
    Transitions { style: String, seconds: f64 },
    VectorGray,
    Hairlines { minimum: f64 },
    PrinterMarks { margin: f64 },
}

impl Settings {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Transitions { .. } => "Set page transitions",
            Self::VectorGray => "Convert vectors to grayscale",
            Self::Hairlines { .. } => "Fix hairlines",
            Self::PrinterMarks { .. } => "Add printer marks",
        }
    }
}

fn entries(doc: &Document, page: &Dict) -> Vec<Object> {
    page.get(b"Contents")
        .map(|o| match &*doc.resolve(o) {
            Object::Array(a) => a.clone(),
            _ => vec![o.clone()],
        })
        .unwrap_or_default()
}

fn rewrite(
    doc: &mut Document,
    object: &Object,
    inherited: &Dict,
    settings: &Settings,
    seen: &mut BTreeSet<ObjRef>,
    budget: &mut usize,
) -> Result<(Object, Dict), EditError> {
    if *budget == 0 || seen.len() >= 32 {
        return Err(EditError::Invalid("内容图形嵌套过深或数量过多，未改动文档".into()));
    }
    *budget -= 1;
    let reference = object.as_ref();
    if reference.is_some_and(|r| !seen.insert(r)) {
        return Err(EditError::Invalid("内容图形存在循环引用，未改动文档".into()));
    }
    let Object::Stream(mut s) = (*doc.resolve(object)).clone() else {
        return Err(EditError::Invalid("内容流无效".into()));
    };
    let data = s.decoded()?;
    if data.len() > 16 * 1024 * 1024 {
        return Err(EditError::Invalid("内容流过大，请分批处理".into()));
    }
    let mut parsed = parse(&data);
    if parsed.skipped > 0 {
        return Err(EditError::Invalid("内容流不能无损读取，未改动文档".into()));
    }
    let mut changed = false;
    let mut resources = s.dict.get(b"Resources").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_else(|| inherited.clone());
    let mut xo = resources.get(b"XObject").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
    let mut forms = BTreeSet::new();
    for op in &mut parsed.ops {
        match settings {
            Settings::VectorGray if matches!(op.op.as_slice(), b"rg" | b"RG" | b"k" | b"K") => {
                let values =
                    op.operands.iter().map(Object::as_f64).collect::<Option<Vec<_>>>().ok_or_else(|| EditError::Invalid("颜色数值无效".into()))?;
                if values.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
                    return Err(EditError::Invalid("颜色数值超出范围".into()));
                }
                let gray = match values.as_slice() {
                    [r, g, b] => 0.2126 * r + 0.7152 * g + 0.0722 * b,
                    [c, m, y, k] => (1.0 - k) * (1.0 - 0.2126 * c - 0.7152 * m - 0.0722 * y),
                    _ => return Err(EditError::Invalid("颜色操作数无效".into())),
                };
                let stroke = matches!(op.op.as_slice(), b"RG" | b"K");
                *op = Op::new(if stroke { "G" } else { "g" }, vec![num(gray.clamp(0.0, 1.0))]);
                changed = true;
            }
            Settings::Hairlines { minimum } if op.is("w") => {
                let width = op.num(0).ok_or_else(|| EditError::Invalid("线宽数值无效".into()))?;
                if !width.is_finite() || width < 0.0 {
                    return Err(EditError::Invalid("线宽数值无效".into()));
                }
                if width < *minimum {
                    *op = Op::new("w", vec![num(*minimum)]);
                    changed = true;
                }
            }
            _ => {}
        }
        if op.is("Do")
            && let Some(name) = op.name(0)
            && forms.insert(name.to_vec())
            && let Some(o) = xo.get(name).cloned()
            && matches!(&*doc.resolve(&o),Object::Stream(s) if s.dict.name(b"Subtype")==Some(b"Form"))
        {
            let (replacement, _) = rewrite(doc, &o, &resources, settings, seen, budget)?;
            if replacement != o {
                xo.set(name.to_vec(), replacement);
                changed = true;
            }
        }
    }
    if let Some(r) = reference {
        seen.remove(&r);
    }
    if !changed {
        return Ok((object.clone(), resources));
    }
    if !forms.is_empty() {
        resources.set(b"XObject".to_vec(), Object::Dict(xo));
    }
    if s.dict.name(b"Subtype") == Some(b"Form") {
        s.dict.set(b"Resources".to_vec(), Object::Dict(resources.clone()));
    }
    s.dict.remove(b"Filter");
    s.dict.remove(b"DecodeParms");
    Ok((Object::Ref(doc.add(Object::Stream(Stream::flate(s.dict, &serialize_ops(&parsed.ops))))), resources))
}

pub fn apply(doc: &mut Document, pages: &[usize], settings: &Settings) -> Result<(), EditError> {
    if pages.is_empty() || pages.len() > 500 {
        return Err(EditError::Invalid("请选择 1 至 500 页".into()));
    }
    match settings {
        Settings::Transitions { style, seconds }
            if !["none", "Dissolve", "Fade", "Wipe"].contains(&style.as_str()) || !seconds.is_finite() || !(0.1..=30.0).contains(seconds) =>
        {
            return Err(EditError::Invalid("页面切换参数无效".into()));
        }
        Settings::Hairlines { minimum } if !minimum.is_finite() || !(0.01..=10.0).contains(minimum) => {
            return Err(EditError::Invalid("线宽须在 0.01 至 10 点之间".into()));
        }
        Settings::PrinterMarks { margin } if !margin.is_finite() || !(18.0..=144.0).contains(margin) => {
            return Err(EditError::Invalid("印刷标记边距须在 18 至 144 点之间".into()));
        }
        _ => {}
    }
    let all = printcraft_model::pages(doc);
    let mut unique = BTreeSet::new();
    let mut budget = 10000;
    for &pi in pages {
        if !unique.insert(pi) {
            return Err(EditError::Invalid("页码重复".into()));
        }
        let page = all.get(pi).ok_or(EditError::NoSuchPage(pi))?;
        match settings {
            Settings::Transitions { style, seconds } => {
                doc.update_dict(page.obj, |d| {
                    if style == "none" {
                        d.remove(b"Trans");
                    } else {
                        let mut t = Dict::new();
                        t.set(b"S".to_vec(), Object::name(style));
                        t.set(b"D".to_vec(), num(*seconds));
                        d.set(b"Trans".to_vec(), Object::Dict(t));
                    }
                })?;
            }
            Settings::PrinterMarks { margin: m } => {
                let [x0, y0, x1, y1] = page.crop(doc);
                let mut data = String::from("q 0 G 0.5 w\n");
                for (x, y, sx, sy) in [(x0, y0, -1.0, -1.0), (x0, y1, -1.0, 1.0), (x1, y0, 1.0, -1.0), (x1, y1, 1.0, 1.0)] {
                    data.push_str(&format!(
                        "{} {y} m {} {y} l S {x} {} m {x} {} l S\n",
                        x + sx * 6.0,
                        x + sx * (m - 6.0),
                        y + sy * 6.0,
                        y + sy * (m - 6.0)
                    ));
                }
                data.push_str("Q\n");
                crate::stamp(doc, pi, "PrinterMarks", data.into_bytes())?;
                let original = Object::Array([x0, y0, x1, y1].into_iter().map(num).collect());
                let expanded = Object::Array([x0 - m, y0 - m, x1 + m, y1 + m].into_iter().map(num).collect());
                doc.update_dict(page.obj, |d| {
                    d.set(b"TrimBox".to_vec(), original);
                    d.set(b"MediaBox".to_vec(), expanded.clone());
                    d.set(b"CropBox".to_vec(), expanded);
                })?;
            }
            _ => {
                let mut resources = page.dict.get(b"Resources").map(|o| doc.resolve(o)).and_then(|o| o.as_dict().cloned()).unwrap_or_default();
                let mut list = Vec::new();
                for o in entries(doc, &page.dict) {
                    let (new, res) = rewrite(doc, &o, &resources, settings, &mut BTreeSet::new(), &mut budget)?;
                    resources = res;
                    list.push(new);
                }
                doc.update_dict(page.obj, |d| {
                    d.set(b"Contents".to_vec(), Object::Array(list));
                    d.set(b"Resources".to_vec(), Object::Dict(resources));
                })?;
            }
        }
    }
    Ok(())
}
