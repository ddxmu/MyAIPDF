//! Build the artwork layer for positioned Word text. Call only on a private COS copy.
//! Do not delete text operators: invisible rendering preserves their implicit advances.

use crate::{EditError, TextLine, text_lines};
use printcraft_content::{Matrix, Op, parse, serialize_ops};
use printcraft_cos::{Dict, Document, Object, Stream};

pub struct ExportText {
    pub line: TextLine,
    /// Display space, top-left origin, points.
    pub rect: [f64; 4],
    pub size: f64,
}

/// Keep rotated, undecodable, clipped and transparent text in the artwork. Only opaque,
/// horizontal fill text with known coordinates is moved into editable Word text boxes.
pub fn hide_editable_text(doc: &mut Document, page_index: usize) -> Result<Vec<ExportText>, EditError> {
    let page = printcraft_model::pages(doc).into_iter().nth(page_index).ok_or(EditError::NoSuchPage(page_index))?;
    let view = Matrix(page.view_matrix(doc)).invert().ok_or_else(|| EditError::Invalid("页面坐标无效".into()))?;
    let (width, height) = page.display_size(doc);
    let resources = page.dict.get(b"Resources").and_then(|r| doc.resolve(r).as_dict().cloned()).unwrap_or_default();
    let states = resources.get(b"ExtGState").and_then(|r| doc.resolve(r).as_dict().cloned()).unwrap_or_default();
    let mut entries = match page.dict.get(b"Contents") {
        Some(c) => match &*doc.resolve(c) {
            Object::Array(a) => a.clone(),
            _ => vec![c.clone()],
        },
        None => Vec::new(),
    };
    if entries.len() > 512 {
        return Err(EditError::Invalid("页面内容过多，请使用原样保真导出".into()));
    }
    // Interpret one logical page stream: CTM, fonts and text state can span physical streams.
    // This normalization touches only the private export copy, never shared source streams.
    if entries.len() > 1 {
        let mut data = Vec::new();
        for entry in &entries {
            let bytes = match &*doc.resolve(entry) {
                Object::Stream(s) => s.decoded()?,
                _ => return Err(EditError::Invalid("页面内容流无效".into())),
            };
            if data.len().saturating_add(bytes.len()).saturating_add(1) > 16 * 1024 * 1024 {
                return Err(EditError::Invalid("页面内容过大，请使用原样保真导出".into()));
            }
            data.extend_from_slice(&bytes);
            data.push(b'\n');
        }
        let mut dict = Dict::new();
        dict.set(b"Length".to_vec(), data.len() as i64);
        entries = vec![Object::Ref(doc.add(Object::Stream(Stream::from_raw(dict, data))))];
        doc.update_dict(page.obj, |d| d.set(b"Contents".to_vec(), Object::Array(entries.clone())))?;
    }
    let lines = text_lines(doc, page_index)?;
    if lines.len() > 20_000 {
        return Err(EditError::Invalid("页面文字过多，请使用原样保真导出".into()));
    }
    let mut kept = Vec::new();
    let mut streams = Vec::new();
    // A page's content streams share graphics state, including q/Q pairs across streams.
    let mut state = (0.0, true, true, true, Matrix::IDENTITY);
    let mut stack = Vec::new();
    let mut path_rect = None;
    let mut path_present = false;
    for (stream_index, entry) in entries.iter().enumerate() {
        let data = match &*doc.resolve(entry) {
            Object::Stream(s) => s.decoded()?,
            _ => return Err(EditError::Invalid("页面内容流无效".into())),
        };
        if data.len() > 16 * 1024 * 1024 {
            return Err(EditError::Invalid("页面内容过大，请使用原样保真导出".into()));
        }
        let parsed = parse(&data);
        if parsed.skipped != 0 {
            return Err(EditError::Invalid("页面内容无法完整解析，请使用原样保真导出".into()));
        }
        let ops = parsed.ops;
        // (render mode, opaque normal blending, known fill, no partial clip, CTM).
        let mut safe = vec![false; ops.len()];
        for (i, op) in ops.iter().enumerate() {
            match op.op.as_slice() {
                b"q" => {
                    if stack.len() >= 1024 {
                        return Err(EditError::Invalid("页面图形状态过深".into()));
                    }
                    stack.push(state);
                }
                b"Q" => {
                    state = stack.pop().ok_or_else(|| EditError::Invalid("页面图形状态不完整".into()))?;
                }
                b"Tr" => state.0 = op.num(0).unwrap_or(-1.0),
                b"cm" => {
                    if let Some(m) = op.nums::<6>() {
                        state.4 = Matrix(m).then(&state.4);
                    }
                }
                b"re" => {
                    path_rect = if path_present {
                        None
                    } else {
                        op.nums::<4>().map(|[x, y, w, h]| view.bbox(state.4.bbox([x.min(x + w), y.min(y + h), x.max(x + w), y.max(y + h)])))
                    };
                    path_present = true;
                }
                b"m" | b"l" | b"c" | b"v" | b"y" | b"h" => {
                    path_rect = None;
                    path_present = true;
                }
                b"W" | b"W*" => {
                    // A rectangular page-wide clip is just the original page boundary.
                    // Other clipping paths stay rasterized rather than exposing hidden text.
                    state.3 &= path_rect.is_some_and(|r: [f64; 4]| {
                        r.iter().all(|v| v.is_finite()) && r[0] <= 0.001 && r[1] <= 0.001 && r[2] >= width - 0.001 && r[3] >= height - 0.001
                    });
                }
                b"n" | b"S" | b"s" | b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                    path_rect = None;
                    path_present = false;
                }
                b"g" | b"rg" | b"k" => state.2 = true,
                b"cs" => state.2 = matches!(op.name(0), Some(b"DeviceRGB" | b"DeviceGray" | b"DeviceCMYK")),
                b"gs" => {
                    let gs = op.name(0).and_then(|name| states.get(name)).and_then(|o| doc.resolve(o).as_dict().cloned());
                    state.1 = gs.is_some_and(|d| {
                        let opaque = d.get(b"ca").and_then(Object::as_f64).is_none_or(|a| (a - 1.0).abs() < 1e-6);
                        let mask = d.get(b"SMask").is_none_or(|o| o.as_name() == Some(b"None"));
                        let blend = d.get(b"BM").is_none_or(|o| o.as_name() == Some(b"Normal"));
                        state.1 && opaque && mask && blend
                    });
                }
                b"Tj" | b"TJ" | b"'" | b"\"" => safe[i] = state.0 == 0.0 && state.1 && state.2 && state.3,
                _ => {}
            }
        }
        let mut hide = vec![false; ops.len()];
        for line in &lines {
            let (source, indices, _, matrix) = line.source_ops();
            if source != stream_index || !line.decodable || indices.iter().any(|i| !safe.get(*i).copied().unwrap_or(false)) {
                continue;
            }
            // Symbol-font encodings and non-printing characters are not portable Word text.
            let font = line.base_font.to_ascii_lowercase();
            if font.contains("wingdings") || font.contains("webdings") || font.ends_with("symbol") || line.text.chars().any(char::is_control) {
                continue;
            }
            let m = Matrix(matrix).then(&view).0;
            if m.iter().any(|v| !v.is_finite()) || m[0] <= 0.0 || m[3] <= 0.0 || m[1].abs() > 0.001 || m[2].abs() > 0.001 {
                continue;
            }
            let r = view.bbox(line.rect);
            let rect = [r[0], height - r[3], r[2], height - r[1]];
            if rect.iter().any(|v| !v.is_finite())
                || rect[2] <= rect[0]
                || rect[3] <= rect[1]
                || rect[0] < 0.0
                || rect[1] < 0.0
                || rect[2] > width
                || rect[3] > height
            {
                continue;
            }
            for &i in indices {
                if let Some(v) = hide.get_mut(i) {
                    *v = true;
                }
            }
            let size = line.size * (r[3] - r[1]) / (line.rect[3] - line.rect[1]);
            kept.push(ExportText { line: line.clone(), rect, size });
        }
        let mut background = Vec::new();
        for (i, op) in ops.into_iter().enumerate() {
            if hide.get(i).copied().unwrap_or(false) {
                background.push(Op::new("Tr", vec![Object::Int(3)]));
                background.push(op);
                background.push(Op::new("Tr", vec![Object::Int(0)]));
            } else {
                background.push(op);
            }
        }
        let bytes = serialize_ops(&background);
        let mut dict = Dict::new();
        dict.set(b"Length".to_vec(), bytes.len() as i64);
        streams.push(Object::Ref(doc.add(Object::Stream(Stream::from_raw(dict, bytes)))));
    }
    doc.update_dict(page.obj, |d| {
        d.set(b"Contents".to_vec(), Object::Array(streams));
    })?;
    Ok(kept)
}
