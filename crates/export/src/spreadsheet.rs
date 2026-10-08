//! Page-positioned editable cells, with an explicitly labelled page-image fallback.
//! Fonts are named, not embedded; PDF text is never interpreted as executable formulas.

use crate::{Zip, esc, word::Text};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    Preserve,
    #[default]
    Editable,
}
impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Editable => "editable",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        match s {
            "preserve" => Some(Self::Preserve),
            "editable" => Some(Self::Editable),
            _ => None,
        }
    }
}
pub struct Rule {
    pub points: [f64; 4],
    pub color: [f64; 3],
    pub width: f64,
}
pub struct Fill {
    pub rect: [f64; 4],
    pub color: [f64; 3],
}
pub struct Picture {
    pub rect: [f64; 4],
    pub bytes: Vec<u8>,
    pub ext: &'static str,
    pub alt: &'static str,
}
pub struct Page {
    pub width: f64,
    pub height: f64,
    pub text: Vec<Text>,
    pub rules: Vec<Rule>,
    pub fills: Vec<Fill>,
    pub images: Vec<Picture>,
    /// Some means this entire page is a labelled image, not editable cells.
    pub png: Option<Vec<u8>>,
}
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const D: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

fn rgb(c: [f64; 3]) -> String {
    c.iter().map(|v| format!("{:02X}", (v.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
}
fn col(mut n: usize) -> String {
    let mut v = Vec::new();
    loop {
        v.push((b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    v.into_iter().rev().collect()
}
fn address(x: usize, y: usize) -> String {
    format!("{}{}", col(x), y + 1)
}
fn rect_contains(a: [f64; 4], b: [f64; 4]) -> bool {
    a[0] <= b[0] + 1.5 && a[1] <= b[1] + 1.5 && a[2] >= b[2] - 1.5 && a[3] >= b[3] - 1.5
}

/// Locate a ruled cell around a point. Missing internal lines naturally produce merged cells.
fn table_box(p: &Page, x: f64, y: f64) -> Option<[f64; 4]> {
    let (mut left, mut right, mut top, mut bottom) = (None, None, None, None);
    for r in &p.rules {
        let [x0, y0, x1, y1] = r.points;
        if (x1 - x0).abs() < 0.1 && y >= y0.min(y1) - 0.2 && y <= y0.max(y1) + 0.2 {
            if x0 < x - 0.2 && left.is_none_or(|v| x0 > v) {
                left = Some(x0);
            }
            if x0 > x + 0.2 && right.is_none_or(|v| x0 < v) {
                right = Some(x0);
            }
        }
        if (y1 - y0).abs() < 0.1 && x >= x0.min(x1) - 0.2 && x <= x0.max(x1) + 0.2 {
            if y0 < y - 0.2 && top.is_none_or(|v| y0 > v) {
                top = Some(y0);
            }
            if y0 > y + 0.2 && bottom.is_none_or(|v| y0 < v) {
                bottom = Some(y0);
            }
        }
    }
    Some([left?, top?, right?, bottom?])
}
struct Cell<'a> {
    rect: [f64; 4],
    runs: Vec<&'a Text>,
    table: bool,
}
fn cells(p: &Page) -> Vec<Cell<'_>> {
    let mut out: Vec<Cell<'_>> = Vec::new();
    let mut lines: Vec<_> = p.text.iter().filter(|t| !t.text.trim().is_empty()).collect();
    lines.sort_by(|a, b| a.rect[1].total_cmp(&b.rect[1]).then(a.rect[0].total_cmp(&b.rect[0])));
    for t in lines {
        let table = table_box(p, (t.rect[0] + t.rect[2]) / 2.0, (t.rect[1] + t.rect[3]) / 2.0).filter(|r| rect_contains(*r, t.rect));
        if let Some(r) = table {
            if let Some(c) = out.iter_mut().find(|c| c.table && c.rect.iter().zip(r).all(|(a, b)| (a - b).abs() < 0.2)) {
                c.runs.push(t);
            } else {
                out.push(Cell { rect: r, runs: vec![t], table: true });
            }
        } else if let Some(c) = out.iter_mut().rev().find(|c| {
            !c.table && (c.rect[1] - t.rect[1]).abs() < t.size * 0.25 && (t.rect[0] - c.rect[2]).max(c.rect[0] - t.rect[2]) <= (t.size * 0.6).max(4.0)
        }) {
            c.rect = [c.rect[0].min(t.rect[0]), c.rect[1].min(t.rect[1]), c.rect[2].max(t.rect[2]), c.rect[3].max(t.rect[3])];
            c.runs.push(t);
        } else {
            out.push(Cell { rect: t.rect, runs: vec![t], table: false });
        }
    }
    for c in &mut out {
        if !c.table {
            c.runs.sort_by(|a, b| a.rect[0].total_cmp(&b.rect[0]));
        }
    }
    // Retain empty ruled cells too; a PDF table is not merely its nonempty labels.
    let mut vertical: Vec<f64> = p.rules.iter().filter(|r| (r.points[0] - r.points[2]).abs() < 0.1).map(|r| r.points[0]).collect();
    vertical.sort_by(f64::total_cmp);
    vertical.dedup_by(|a, b| (*a - *b).abs() < 0.2);
    for rule in p.rules.iter().filter(|r| (r.points[1] - r.points[3]).abs() < 0.1) {
        for x in vertical.windows(2) {
            let mid = (x[0] + x[1]) / 2.0;
            if mid < rule.points[0].min(rule.points[2]) || mid > rule.points[0].max(rule.points[2]) {
                continue;
            }
            if let Some(rect) = table_box(p, mid, rule.points[1] + 0.5)
                && (rect[1] - rule.points[1]).abs() < 0.2
                && !out.iter().any(|c| c.rect.iter().zip(rect).all(|(a, b)| (a - b).abs() < 0.2))
            {
                out.push(Cell { rect, runs: Vec::new(), table: true });
                if out.len() > 2000 {
                    return out;
                }
            }
        }
    }
    out
}
fn axis(mut v: Vec<f64>, end: f64, chunk: f64) -> Vec<f64> {
    let mut n = chunk;
    while n < end {
        v.push(n);
        n += chunk;
    }
    v.extend([0.0, end]);
    v.retain(|n| n.is_finite() && *n >= 0.0 && *n <= end);
    v.sort_by(f64::total_cmp);
    v.dedup_by(|a, b| (*a - *b).abs() < 0.6);
    v
}
fn edge(v: &[f64], n: f64) -> usize {
    v.iter().enumerate().min_by(|(_, a), (_, b)| (*a - n).abs().total_cmp(&(*b - n).abs())).map_or(0, |(i, _)| i)
}
fn run_xml(t: &Text, text: &str) -> String {
    format!(
        "<r><rPr><rFont val=\"{}\"/><sz val=\"{:.3}\"/>{}{}<color rgb=\"FF{}\"/></rPr><t xml:space=\"preserve\">{}</t></r>",
        esc(&crate::word::family(&t.font)),
        t.size,
        if t.bold { "<b/>" } else { "" },
        if t.italic { "<i/>" } else { "" },
        rgb(t.color),
        esc(text)
    )
}
fn cell_text(c: &Cell<'_>) -> String {
    let mut s = String::new();
    let mut previous: Option<&Text> = None;
    for t in &c.runs {
        if let Some(prev) = previous {
            s.push_str(&separator(prev, t));
        }
        s.push_str(&t.text);
        previous = Some(t);
    }
    s
}
fn separator(previous: &Text, next: &Text) -> String {
    let dy = (previous.rect[1] - next.rect[1]).abs();
    if dy > next.size * 0.4 {
        "\n".repeat((dy / (next.size * 1.25)).round().clamp(1.0, 8.0) as usize)
    } else if next.rect[0] - previous.rect[2] > next.size * 0.25 {
        " ".into()
    } else {
        String::new()
    }
}
/// Convert only unambiguous decimal amounts, preserving precision and leading-zero identifiers.
fn numeric(text: &str) -> Option<(f64, String)> {
    if text.trim() != text || text.is_empty() || text.len() > 24 {
        return None;
    }
    let percentage = text.ends_with('%');
    let raw = text.strip_suffix('%').unwrap_or(text);
    let digits = raw.trim_start_matches(['-', '+']);
    if digits.starts_with('0') && digits.len() > 1 && !digits.starts_with("0.") {
        return None;
    }
    let mut parts = digits.split('.');
    let integer = parts.next()?;
    let decimal = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|b| b.is_ascii_digit())
        || decimal.is_some_and(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
        || integer.len().saturating_add(decimal.map_or(0, str::len)) > 15
    {
        return None;
    }
    let mut value = raw.parse::<f64>().ok()?;
    if !value.is_finite() {
        return None;
    }
    if percentage {
        value /= 100.0;
    }
    let format = format!("0{}{}", decimal.map_or(String::new(), |d| format!(".{}", "0".repeat(d.len()))), if percentage { "%" } else { "" });
    Some((value, format))
}
fn relation(id: &str, kind: &str, target: &str) -> String {
    format!("<Relationship Id=\"{id}\" Type=\"{R}/{kind}\" Target=\"{target}\"/>")
}
fn anchor(rect: [f64; 4], shape: &str, xs: &[f64], ys: &[f64]) -> String {
    let marker = |tag: &str, x: f64, y: f64| {
        let col = xs.windows(2).position(|w| x >= w[0] && x < w[1]).unwrap_or(xs.len().saturating_sub(2));
        let row = ys.windows(2).position(|w| y >= w[0] && y < w[1]).unwrap_or(ys.len().saturating_sub(2));
        let xo = (x - xs.get(col).copied().unwrap_or(0.0)) * 12700.0;
        let yo = (y - ys.get(row).copied().unwrap_or(0.0)) * 12700.0;
        format!(
            "<xdr:{tag}><xdr:col>{col}</xdr:col><xdr:colOff>{xo:.0}</xdr:colOff><xdr:row>{row}</xdr:row><xdr:rowOff>{yo:.0}</xdr:rowOff></xdr:{tag}>"
        )
    };
    format!("<xdr:twoCellAnchor>{}{}{shape}<xdr:clientData/></xdr:twoCellAnchor>", marker("from", rect[0], rect[1]), marker("to", rect[2], rect[3]))
}

struct Grid<'a> {
    cells: Vec<Cell<'a>>,
    xs: Vec<f64>,
    ys: Vec<f64>,
    boxes: Vec<[usize; 4]>,
}
fn grid(p: &Page) -> Result<Grid<'_>, String> {
    let valid_rect = |r: [f64; 4]| {
        r.iter().all(|n| n.is_finite())
            && r[0] >= -0.1
            && r[1] >= -0.1
            && r[2] <= p.width + 0.1
            && r[3] <= p.height + 0.1
            && r[2] > r[0]
            && r[3] > r[1]
    };
    let color = |c: [f64; 3]| c.iter().all(|v| v.is_finite());
    if !p.width.is_finite()
        || !p.height.is_finite()
        || !(7.2..=1584.0).contains(&p.width)
        || !(7.2..=1584.0).contains(&p.height)
        || p.text.len() > 2000
        || p.rules.len() > 2000
        || p.fills.len() > 256
        || p.images.len() > 256
        || p.text.iter().any(|t| {
            !valid_rect(t.rect)
                || !t.size.is_finite()
                || !(1.0..=409.0).contains(&t.size)
                || !color(t.color)
                || t.font.len() > 4096
                || t.font.chars().any(char::is_control)
                || t.text.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        })
        || p.fills.iter().any(|f| !valid_rect(f.rect) || !color(f.color))
        || p.images.iter().any(|im| !valid_rect(im.rect) || !matches!(im.ext, "png" | "jpg"))
        || p.rules.iter().any(|r| r.points.iter().any(|v| !v.is_finite()) || !r.width.is_finite() || r.width < 0.0 || !color(r.color))
    {
        return Err("Excel 页面尺寸、坐标或对象数量超出支持范围".into());
    }
    let horizontal = p.rules.iter().filter(|r| (r.points[1] - r.points[3]).abs() < 0.1).count();
    let vertical = p.rules.iter().filter(|r| (r.points[0] - r.points[2]).abs() < 0.1).count();
    if horizontal.saturating_mul(vertical).saturating_mul(p.rules.len()) > 10_000_000 {
        return Err("Excel 表格过密".into());
    }
    let mut cs = cells(p);
    if cs.len() > 2000 {
        return Err("Excel 单页单元格过多".into());
    }
    for c in &mut cs {
        if !c.table {
            c.rect[2] = (c.rect[2] + 4.0).min(p.width);
            c.rect[3] = (c.rect[3] + 1.0).min(p.height);
        }
        if !valid_rect(c.rect) || cell_text(c).encode_utf16().count() > 32767 {
            return Err("Excel 单元格坐标或文字超出支持范围".into());
        }
    }
    let xs =
        axis(cs.iter().flat_map(|c| [c.rect[0], c.rect[2]]).chain(p.fills.iter().flat_map(|f| [f.rect[0], f.rect[2]])).collect(), p.width, 200.0);
    let ys =
        axis(cs.iter().flat_map(|c| [c.rect[1], c.rect[3]]).chain(p.fills.iter().flat_map(|f| [f.rect[1], f.rect[3]])).collect(), p.height, 300.0);
    if xs.len() > 1024
        || ys.len() > 4096
        || xs.len().saturating_mul(ys.len()) > 250_000
        || (!p.fills.is_empty() && xs.len().saturating_mul(ys.len()).saturating_mul(cs.len().saturating_add(p.fills.len())) > 10_000_000)
    {
        return Err("Excel 页面布局过密".into());
    }
    let mut boxes = Vec::<[usize; 4]>::new();
    for c in &cs {
        let r = [edge(&xs, c.rect[0]), edge(&ys, c.rect[1]), edge(&xs, c.rect[2]), edge(&ys, c.rect[3])];
        if r[2] <= r[0] || r[3] <= r[1] || boxes.iter().any(|a| r[0] < a[2] && r[2] > a[0] && r[1] < a[3] && r[3] > a[1]) {
            return Err("Excel 单元格过小或重叠".into());
        }
        boxes.push(r);
    }
    Ok(Grid { cells: cs, xs, ys, boxes })
}
/// The engine falls back only the unsupported page, not the entire document.
pub fn validate(p: &Page) -> Result<(), String> {
    grid(p).map(|_| ())
}
fn border(p: &Page, r: [f64; 4]) -> String {
    let mut b = String::from("<border>");
    for (side, vertical, v, start, end) in
        [("left", true, r[0], r[1], r[3]), ("right", true, r[2], r[1], r[3]), ("top", false, r[1], r[0], r[2]), ("bottom", false, r[3], r[0], r[2])]
    {
        let rule = p.rules.iter().find(|rule| {
            let q = rule.points;
            let (a, z, s, e) = if vertical { (q[0], q[2], q[1], q[3]) } else { (q[1], q[3], q[0], q[2]) };
            (a - z).abs() < 0.1 && (a - v).abs() < 0.2 && s.min(e) <= start + 0.2 && s.max(e) >= end - 0.2
        });
        if let Some(rule) = rule {
            b.push_str(&format!(
                "<{side} style=\"{}\"><color rgb=\"FF{}\"/></{side}>",
                if rule.width < 0.4 {
                    "hair"
                } else if rule.width < 1.25 {
                    "thin"
                } else if rule.width < 2.5 {
                    "medium"
                } else {
                    "thick"
                },
                rgb(rule.color)
            ));
        } else {
            b.push_str(&format!("<{side}/>"));
        }
    }
    b.push_str("<diagonal/></border>");
    b
}

pub fn xlsx(pages: &[Page]) -> Result<Vec<u8>, String> {
    if pages.is_empty() || pages.len() > 500 {
        return Err("Excel 导出支持 1 至 500 页，请分批导出".into());
    }
    let mut zip = Zip::default();
    let mut types = format!(
        "<Types xmlns=\"{CT}\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/><Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>"
    );
    let mut book = format!("<workbook xmlns=\"{S}\" xmlns:r=\"{R}\"><bookViews><workbookView/></bookViews><sheets>");
    let mut book_rels = relation("styles", "styles", "styles.xml");
    let (mut fonts, mut fills, mut borders, mut formats, mut styles) = (
        vec!["<font><sz val=\"11\"/><name val=\"Calibri\"/></font>".to_string()],
        vec!["<fill><patternFill patternType=\"none\"/></fill>".to_string(), "<fill><patternFill patternType=\"gray125\"/></fill>".to_string()],
        vec!["<border><left/><right/><top/><bottom/><diagonal/></border>".to_string()],
        Vec::<String>::new(),
        vec!["<xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/>".to_string()],
    );
    let mut total = 0usize;
    let mut image_count = 0usize;
    let mut xml_total = 0usize;
    let mut print_areas = Vec::new();
    for (i, p) in pages.iter().enumerate() {
        let Grid { cells: cs, xs, ys, boxes } = grid(p)?;
        let n = i + 1;
        let name = format!(
            "第 {n} 页{}",
            if p.png.is_some() {
                "（图像）"
            } else if p.images.iter().any(|im| im.alt != "PDF 原始图片") {
                "（局部图像）"
            } else {
                ""
            }
        );
        print_areas.push(format!(
            "<definedName name=\"_xlnm.Print_Area\" localSheetId=\"{i}\">'{name}'!$A$1:${}${}</definedName>",
            col(xs.len().saturating_sub(2)),
            ys.len().saturating_sub(1)
        ));
        book.push_str(&format!("<sheet name=\"{name}\" sheetId=\"{n}\" r:id=\"sheet{n}\"/>"));
        book_rels.push_str(&relation(&format!("sheet{n}"), "worksheet", &format!("worksheets/sheet{n}.xml")));
        let mut sheet = format!(
            "<worksheet xmlns=\"{S}\" xmlns:r=\"{R}\"><sheetPr><pageSetUpPr fitToPage=\"1\"/></sheetPr><dimension ref=\"A1:{}\"/><sheetViews><sheetView workbookViewId=\"0\" showGridLines=\"0\"/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/><cols>",
            address(xs.len().saturating_sub(2), ys.len().saturating_sub(2))
        );
        for (j, w) in xs.windows(2).enumerate() {
            let pixels = (w[1] - w[0]) * 96.0 / 72.0;
            sheet.push_str(&format!(
                "<col min=\"{}\" max=\"{}\" width=\"{:.8}\" customWidth=\"1\"/>",
                j + 1,
                j + 1,
                (pixels / 7.0 * 256.0).round() / 256.0
            ));
        }
        sheet.push_str("</cols><sheetData>");
        let mut rows: BTreeMap<usize, Vec<(usize, String)>> = BTreeMap::new();
        let mut merges = Vec::new();
        for (c, &[x0, y0, x1, y1]) in cs.iter().zip(&boxes) {
            let first = c.runs.first().copied();
            let text = cell_text(c);
            if text.encode_utf16().count() > 32767 {
                return Err("Excel 单元格文字超过上限，请使用原样保真".into());
            }
            let font_id = first.map_or(0, |first| {
                intern(
                    &mut fonts,
                    format!(
                        "<font><sz val=\"{:.3}\"/><name val=\"{}\"/>{}{}<color rgb=\"FF{}\"/></font>",
                        first.size,
                        esc(&crate::word::family(&first.font)),
                        if first.bold { "<b/>" } else { "" },
                        if first.italic { "<i/>" } else { "" },
                        rgb(first.color)
                    ),
                )
            });
            let fill = p.fills.iter().rev().find(|f| rect_contains(f.rect, c.rect)).map(|f| {
                format!(
                    "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{}\"/><bgColor indexed=\"64\"/></patternFill></fill>",
                    rgb(f.color)
                )
            });
            let fill_id = fill.map_or(0, |f| intern(&mut fills, f));
            let border_id = if c.table { intern(&mut borders, border(p, [xs[x0], ys[y0], xs[x0 + 1], ys[y0 + 1]])) } else { 0 };
            let number = numeric(&text);
            let format_id = number.as_ref().map_or(0, |(_, f)| intern(&mut formats, f.clone()) + 164);
            let centered = c.table && first.is_some_and(|first| (first.rect[0] - c.rect[0] - (c.rect[2] - first.rect[2])).abs() < 4.0);
            let style = format!(
                "<xf numFmtId=\"{format_id}\" fontId=\"{font_id}\" fillId=\"{fill_id}\" borderId=\"{border_id}\" xfId=\"0\" applyFont=\"1\" applyFill=\"1\" applyBorder=\"1\" applyNumberFormat=\"1\" applyAlignment=\"1\"><alignment horizontal=\"{}\" vertical=\"{}\" wrapText=\"{}\"/></xf>",
                if centered { "center" } else { "left" },
                if c.table { "center" } else { "top" },
                if c.table { 1 } else { 0 }
            );
            let style_id = intern(&mut styles, style);
            if styles.len() > 20_000 {
                return Err("Excel 样式过多".into());
            }
            let ref_ = address(x0, y0);
            let xml = if let Some((value, _)) = number {
                format!("<c r=\"{ref_}\" s=\"{style_id}\" t=\"n\"><v>{value}</v></c>")
            } else {
                let mut runs = String::new();
                let mut previous: Option<&Text> = None;
                for t in &c.runs {
                    let spacing = previous.map_or_else(String::new, |prev| separator(prev, t));
                    runs.push_str(&run_xml(t, &format!("{spacing}{}", t.text)));
                    previous = Some(t);
                }
                format!("<c r=\"{ref_}\" s=\"{style_id}\" t=\"inlineStr\"><is>{runs}</is></c>")
            };
            rows.entry(y0).or_default().push((x0, xml));
            // Merged ranges need border styles on their perimeter cells, not all four
            // borders on the tiny top-left grid cell (which creates spurious lines).
            if c.table {
                for y in y0..y1 {
                    for x in x0..x1 {
                        if (x == x0 && y == y0) || (x > x0 && x + 1 < x1 && y > y0 && y + 1 < y1) {
                            continue;
                        }
                        let bid = intern(&mut borders, border(p, [xs[x], ys[y], xs[x + 1], ys[y + 1]]));
                        let sid = intern(
                            &mut styles,
                            format!(
                                "<xf numFmtId=\"0\" fontId=\"0\" fillId=\"{fill_id}\" borderId=\"{bid}\" xfId=\"0\" applyFill=\"1\" applyBorder=\"1\"/>"
                            ),
                        );
                        rows.entry(y).or_default().push((x, format!("<c r=\"{}\" s=\"{sid}\"/>", address(x, y))));
                    }
                }
            }
            if x1 > x0 + 1 || y1 > y0 + 1 {
                merges.push(format!("<mergeCell ref=\"{ref_}:{}\"/>", address(x1 - 1, y1 - 1)));
            }
        }
        // Background fills also cover blank grid cells, without opaque shapes over text.
        for (y, h) in ys.windows(2).enumerate().filter(|_| !p.fills.is_empty()) {
            for (x, w) in xs.windows(2).enumerate() {
                if boxes.iter().any(|r| x >= r[0] && x < r[2] && y >= r[1] && y < r[3]) {
                    continue;
                }
                if let Some(f) = p.fills.iter().rev().find(|f| rect_contains(f.rect, [w[0], h[0], w[1], h[1]])) {
                    let fill_id = intern(
                        &mut fills,
                        format!(
                            "<fill><patternFill patternType=\"solid\"><fgColor rgb=\"FF{}\"/><bgColor indexed=\"64\"/></patternFill></fill>",
                            rgb(f.color)
                        ),
                    );
                    let s = intern(
                        &mut styles,
                        format!("<xf numFmtId=\"0\" fontId=\"0\" fillId=\"{fill_id}\" borderId=\"0\" xfId=\"0\" applyFill=\"1\"/>"),
                    );
                    rows.entry(y).or_default().push((x, format!("<c r=\"{}\" s=\"{s}\"/>", address(x, y))));
                }
            }
        }
        for (j, h) in ys.windows(2).enumerate() {
            sheet.push_str(&format!("<row r=\"{}\" ht=\"{:.4}\" customHeight=\"1\">", j + 1, h[1] - h[0]));
            if let Some(row) = rows.get_mut(&j) {
                row.sort_by_key(|c| c.0);
                for (_, xml) in row {
                    sheet.push_str(xml);
                }
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData>");
        if !merges.is_empty() {
            sheet.push_str(&format!("<mergeCells count=\"{}\">{}</mergeCells>", merges.len(), merges.join("")));
        }
        sheet.push_str(&format!("<printOptions gridLines=\"0\" headings=\"0\"/><pageMargins left=\"0\" right=\"0\" top=\"0\" bottom=\"0\" header=\"0\" footer=\"0\"/><pageSetup paperWidth=\"{:.3}pt\" paperHeight=\"{:.3}pt\" orientation=\"{}\" fitToWidth=\"1\" fitToHeight=\"1\"/>",p.width,p.height,if p.width>p.height {"landscape"}else{"portrait"}));
        let mut drawing = format!("<xdr:wsDr xmlns:xdr=\"{D}\" xmlns:a=\"{A}\" xmlns:r=\"{R}\">");
        let mut drawing_rels = String::new();
        let mut did = 1usize;
        let image_refs: Vec<_> = if let Some(png) = &p.png {
            vec![([0.0, 0.0, p.width, p.height], png, "png", "PDF 原样页面图像，正文不可逐字编辑")]
        } else {
            p.images.iter().map(|im| (im.rect, &im.bytes, im.ext, im.alt)).collect()
        };
        for (rect, bytes, ext, alt) in image_refs {
            image_count = image_count.saturating_add(1);
            total = total.saturating_add(bytes.len());
            if total > 256 * 1024 * 1024 || image_count > 20_000 || !matches!(ext, "png" | "jpg") {
                return Err("Excel 图片过大或格式无效".into());
            }
            let path = format!("page{n}-image{did}.{ext}");
            zip.add(&format!("xl/media/{path}"), bytes, false);
            drawing_rels.push_str(&relation(&format!("img{did}"), "image", &format!("../media/{path}")));
            let shape = format!(
                "<xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{did}\" name=\"图片 {did}\" descr=\"{alt}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"img{did}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{:.0}\" cy=\"{:.0}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic>",
                (rect[2] - rect[0]) * 12700.0,
                (rect[3] - rect[1]) * 12700.0
            );
            drawing.push_str(&anchor(rect, &shape, &xs, &ys));
            did += 1;
        }
        if p.png.is_none() {
            for r in &p.rules {
                let [x0, y0, x1, y1] = r.points;
                let rect = [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)];
                let shape = format!(
                    "<xdr:sp><xdr:nvSpPr><xdr:cNvPr id=\"{did}\" name=\"PDF 线条 {did}\"/><xdr:cNvSpPr/></xdr:nvSpPr><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{:.0}\" cy=\"{:.0}\"/></a:xfrm><a:prstGeom prst=\"line\"><a:avLst/></a:prstGeom><a:ln w=\"{:.0}\"><a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill></a:ln></xdr:spPr></xdr:sp>",
                    ((rect[2] - rect[0]) * 12700.0).max(1.0),
                    ((rect[3] - rect[1]) * 12700.0).max(1.0),
                    r.width.max(0.1) * 12700.0,
                    rgb(r.color)
                );
                drawing.push_str(&anchor(rect, &shape, &xs, &ys));
                did += 1;
            }
        }
        drawing.push_str("</xdr:wsDr>");
        if did > 1 {
            sheet.push_str("<drawing r:id=\"drawing\"/>");
            zip.add(&format!("xl/drawings/drawing{n}.xml"), drawing.as_bytes(), true);
            zip.add(
                &format!("xl/drawings/_rels/drawing{n}.xml.rels"),
                format!("<Relationships xmlns=\"{RELS}\">{drawing_rels}</Relationships>").as_bytes(),
                true,
            );
            zip.add(
                &format!("xl/worksheets/_rels/sheet{n}.xml.rels"),
                format!("<Relationships xmlns=\"{RELS}\">{}</Relationships>", relation("drawing", "drawing", &format!("../drawings/drawing{n}.xml")))
                    .as_bytes(),
                true,
            );
            types.push_str(&format!(
                "<Override PartName=\"/xl/drawings/drawing{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"
            ));
        }
        sheet.push_str("</worksheet>");
        xml_total = xml_total.saturating_add(sheet.len()).saturating_add(drawing.len());
        if xml_total > 64 * 1024 * 1024 {
            return Err("Excel 文字内容过大".into());
        }
        zip.add(&format!("xl/worksheets/sheet{n}.xml"), sheet.as_bytes(), true);
        types.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"));
    }
    book.push_str("</sheets><definedNames>");
    book.push_str(&print_areas.join(""));
    book.push_str("</definedNames></workbook>");
    let styles_xml = format!(
        "<styleSheet xmlns=\"{S}\"><numFmts count=\"{}\">{}</numFmts><fonts count=\"{}\">{}</fonts><fills count=\"{}\">{}</fills><borders count=\"{}\">{}</borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"{}\">{}</cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>",
        formats.len(),
        formats.iter().enumerate().map(|(i, f)| format!("<numFmt numFmtId=\"{}\" formatCode=\"{}\"/>", i + 164, esc(f))).collect::<String>(),
        fonts.len(),
        fonts.join(""),
        fills.len(),
        fills.join(""),
        borders.len(),
        borders.join(""),
        styles.len(),
        styles.join("")
    );
    types.push_str("</Types>");
    zip.add("[Content_Types].xml", types.as_bytes(), true);
    zip.add(
        "_rels/.rels",
        format!("<Relationships xmlns=\"{RELS}\">{}</Relationships>", relation("book", "officeDocument", "xl/workbook.xml")).as_bytes(),
        true,
    );
    zip.add("xl/workbook.xml", book.as_bytes(), true);
    zip.add("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{RELS}\">{book_rels}</Relationships>").as_bytes(), true);
    zip.add("xl/styles.xml", styles_xml.as_bytes(), true);
    Ok(zip.finish())
}
fn intern(v: &mut Vec<String>, s: String) -> usize {
    if let Some(i) = v.iter().position(|x| x == &s) {
        i
    } else {
        v.push(s);
        v.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::part;

    fn page() -> Page {
        let text = |s: &str, rect: [f64; 4]| Text {
            text: s.into(),
            rect,
            size: 12.0,
            font: "ABCDEF+SimSun".into(),
            bold: false,
            italic: false,
            color: [0.2, 0.3, 0.4],
        };
        Page {
            width: 200.0,
            height: 160.0,
            text: vec![
                text("项目", [20.0, 40.0, 60.0, 55.0]),
                text("00123", [20.0, 80.0, 60.0, 95.0]),
                text("=1+1", [90.0, 80.0, 140.0, 95.0]),
                text("说明", [20.0, 125.0, 120.0, 140.0]),
            ],
            rules: vec![
                [10.0, 30.0, 170.0, 30.0],
                [10.0, 70.0, 170.0, 70.0],
                [10.0, 110.0, 170.0, 110.0],
                [10.0, 30.0, 10.0, 110.0],
                [80.0, 30.0, 80.0, 110.0],
                [170.0, 30.0, 170.0, 110.0],
            ]
            .into_iter()
            .map(|points| Rule { points, color: [0.2, 0.4, 0.6], width: 1.5 })
            .collect(),
            fills: vec![Fill { rect: [10.0, 30.0, 170.0, 70.0], color: [0.9, 0.95, 1.0] }],
            images: Vec::new(),
            png: None,
        }
    }

    #[test]
    fn cells_keep_source_geometry_empty_tables_fonts_colors_and_literal_formulas() {
        let p = page();
        let g = grid(&p).unwrap();
        assert_eq!(g.cells.len(), 5);
        assert_eq!(g.cells.iter().filter(|c| c.runs.is_empty()).count(), 1);
        let perimeter = border(&p, [10.0, 30.0, 20.0, 50.0]);
        assert!(perimeter.contains("<right/>") && perimeter.contains("<bottom/>") && perimeter.contains("<left style=\"medium\""));
        let bytes = xlsx(std::slice::from_ref(&p)).unwrap();
        let sheet = part(&bytes, "xl/worksheets/sheet1.xml");
        let style = part(&bytes, "xl/styles.xml");
        assert!(sheet.contains("showGridLines=\"0\"") && sheet.contains("<mergeCells") && sheet.contains("paperWidth=\"200.000pt\""));
        assert!(sheet.contains("00123") && sheet.contains("=1+1") && !sheet.contains("<f>"));
        assert!(
            style.contains("name val=\"SimSun\"") && style.contains("FF334D66") && style.contains("FF336699") && style.contains("style=\"medium\"")
        );
        let range = format!("'第 1 页'!$A$1:${}${}", col(g.xs.len() - 2), g.ys.len() - 1);
        assert!(part(&bytes, "xl/workbook.xml").contains(&range));
        assert!(!part(&bytes, "xl/_rels/workbook.xml.rels").contains("External"));
    }

    #[test]
    fn decimal_amounts_are_numeric_but_ids_and_formula_text_are_not() {
        assert_eq!(numeric("123.40"), Some((123.4, "0.00".into())));
        assert_eq!(numeric("12.5%"), Some((0.125, "0.0%".into())));
        for literal in ["00123", "1234567890123456", "=SUM(A1:A2)", "-1E2", "2026-10-08", " 4"] {
            assert!(numeric(literal).is_none());
        }
        let mut p = page();
        p.text[2].text = "123.40".into();
        let bytes = xlsx(&[p]).unwrap();
        assert!(part(&bytes, "xl/worksheets/sheet1.xml").contains("t=\"n\"><v>123.4</v>"));
        assert!(part(&bytes, "xl/styles.xml").contains("formatCode=\"0.00\""));
    }

    #[test]
    fn page_image_mode_is_explicit_and_bad_geometry_is_rejected() {
        let p = Page {
            width: 200.0,
            height: 160.0,
            text: Vec::new(),
            rules: Vec::new(),
            fills: Vec::new(),
            images: Vec::new(),
            png: Some(vec![1, 2, 3]),
        };
        let bytes = xlsx(&[p]).unwrap();
        assert!(part(&bytes, "xl/workbook.xml").contains("第 1 页（图像）"));
        assert!(part(&bytes, "xl/drawings/drawing1.xml").contains("正文不可逐字编辑"));
        assert!(part(&bytes, "xl/drawings/drawing1.xml").contains("<xdr:twoCellAnchor>"));
        let mut p = page();
        p.text[0].rect[0] = f64::NAN;
        assert!(validate(&p).is_err());
        let mut p = page();
        p.text.push(Text {
            text: "overlap".into(),
            rect: [12.0, 35.0, 100.0, 90.0],
            size: 12.0,
            font: "Inter".into(),
            bold: false,
            italic: false,
            color: [0.0; 3],
        });
        assert!(validate(&p).is_err());
        assert!(xlsx(&[]).is_err());
        assert_eq!(Mode::default(), Mode::Editable);
        assert_eq!(Mode::from_id("image"), None);
    }
}
