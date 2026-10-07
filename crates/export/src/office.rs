//! Minimal, self-contained SpreadsheetML and image-slide PresentationML packages.
//! XLSX is extracted text/tables, not a layout clone. PPTX deliberately uses page images
//! to preserve appearance; it does not pretend to reconstruct editable source slides.

use crate::{Page, Zip, esc, tables};

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const REL_NS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT_NS: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";

fn rels(entries: &[(&str, String, String)]) -> String {
    format!(
        "<Relationships xmlns=\"{REL_NS}\">{}</Relationships>",
        entries.iter().map(|(id, kind, path)| format!("<Relationship Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{path}\"/>")).collect::<String>()
    )
}

fn col(mut n: usize) -> String {
    let mut chars = Vec::new();
    loop {
        chars.push((b'A' + (n % 26) as u8) as char);
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    chars.into_iter().rev().collect()
}

pub fn xlsx(pages: &[Page]) -> Result<Vec<u8>, String> {
    if pages.is_empty() || pages.len() > 1000 {
        return Err("Excel 导出支持 1 至 1000 页，请分批导出".into());
    }
    let mut zip = Zip::default();
    let mut types = format!(
        "<Types xmlns=\"{CT_NS}\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>"
    );
    let mut book = format!("<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"{REL}\"><sheets>");
    let mut relationships = String::new();
    for (i, p) in pages.iter().enumerate() {
        let n = i + 1;
        let (found, used) = tables(&p.blocks);
        let mut rows: Vec<Vec<String>> = Vec::new();
        for t in found {
            for row in t.rows {
                let mut cells = Vec::new();
                for c in row {
                    cells.push(c.text);
                    cells.extend((1..c.span).map(|_| String::new()));
                }
                rows.push(cells);
            }
            rows.push(Vec::new());
        }
        let mut paragraphs: Vec<_> = p.blocks.iter().enumerate().filter(|(j, _)| !used.get(*j).copied().unwrap_or(false)).map(|(_, b)| b).collect();
        paragraphs.sort_by(|a, b| b.rect[3].total_cmp(&a.rect[3]).then(a.rect[0].total_cmp(&b.rect[0])));
        for b in paragraphs {
            for line in b.text.lines() {
                rows.push(vec![line.to_owned()]);
            }
        }
        let mut sheet = "<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>".to_string();
        if rows.len() > 1_048_576 {
            return Err("Excel 行数超出格式限制，请缩小导出范围".into());
        }
        for (j, row) in rows.iter().enumerate() {
            sheet.push_str(&format!("<row r=\"{}\">", j + 1));
            if row.len() > 16384 {
                return Err("Excel 列数超出格式限制".into());
            }
            for (k, text) in row.iter().enumerate() {
                if text.encode_utf16().count() > 32767 {
                    return Err("某个段落超过 Excel 单元格文字上限，请先拆分段落".into());
                }
                sheet.push_str(&format!("<c r=\"{}{}\" t=\"inlineStr\"><is><t xml:space=\"preserve\">{}</t></is></c>", col(k), j + 1, esc(text)));
            }
            sheet.push_str("</row>");
        }
        sheet.push_str("</sheetData></worksheet>");
        zip.add(&format!("xl/worksheets/sheet{n}.xml"), sheet.as_bytes(), true);
        types.push_str(&format!("<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"));
        book.push_str(&format!("<sheet name=\"第 {n} 页\" sheetId=\"{n}\" r:id=\"rId{n}\"/>"));
        relationships.push_str(&format!("<Relationship Id=\"rId{n}\" Type=\"{REL}/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"));
    }
    types.push_str("</Types>");
    book.push_str("</sheets></workbook>");
    zip.add("[Content_Types].xml", types.as_bytes(), true);
    zip.add("_rels/.rels", rels(&[("rId1", "officeDocument".into(), "xl/workbook.xml".into())]).as_bytes(), true);
    zip.add("xl/workbook.xml", book.as_bytes(), true);
    zip.add("xl/_rels/workbook.xml.rels", format!("<Relationships xmlns=\"{REL_NS}\">{relationships}</Relationships>").as_bytes(), true);
    Ok(zip.finish())
}

fn group() -> &'static str {
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"0\" cy=\"0\"/></a:xfrm></p:grpSpPr>"
}

/// (page width, page height, PNG). Aspect ratio is retained, letterboxed on the first page's size.
pub fn pptx(pages: &[(f64, f64, Vec<u8>)]) -> Result<Vec<u8>, String> {
    if pages.is_empty() || pages.len() > 500 {
        return Err("PPT 导出支持 1 至 500 页，请分批导出".into());
    }
    if pages.iter().any(|(w, h, _)| !w.is_finite() || !h.is_finite() || *w <= 0.0 || *h <= 0.0) {
        return Err("PDF 页面尺寸无效".into());
    }
    let (sw, sh) = (pages[0].0.clamp(72.0, 2880.0) * 12700.0, pages[0].1.clamp(72.0, 2880.0) * 12700.0);
    let mut zip = Zip::default();
    let mut types = format!(
        "<Types xmlns=\"{CT_NS}\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/>"
    );
    for (path, ctype) in [
        ("presentation.xml", "presentation.main"),
        ("slideMasters/slideMaster1.xml", "slideMaster"),
        ("slideLayouts/slideLayout1.xml", "slideLayout"),
        ("presProps.xml", "presProps"),
    ] {
        types.push_str(&format!(
            "<Override PartName=\"/ppt/{path}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.{ctype}+xml\"/>"
        ));
    }
    types.push_str("<Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>");
    let mut ids = String::new();
    let mut presentation_rels =
        vec![("rId1", "slideMaster".into(), "slideMasters/slideMaster1.xml".into()), ("rId2", "presProps".into(), "presProps.xml".into())];
    let mut slide_rel_ids = Vec::new();
    for (i, (w, h, png)) in pages.iter().enumerate() {
        let n = i + 1;
        let id = format!("rId{}", i + 3);
        ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"{id}\"/>", i + 256));
        slide_rel_ids.push(id);
        let scale = (sw / w).min(sh / h);
        let (cx, cy) = (w * scale, h * scale);
        let (x, y) = ((sw - cx) / 2.0, (sh - cy) / 2.0);
        let slide = format!(
            "<p:sld xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{REL}\"><p:cSld><p:spTree>{}<p:pic><p:nvPicPr><p:cNvPr id=\"2\" name=\"PDF 第 {n} 页\" descr=\"PDF 页面图片，不是可编辑文字\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"rId1\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr><a:xfrm><a:off x=\"{:.0}\" y=\"{:.0}\"/><a:ext cx=\"{:.0}\" cy=\"{:.0}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic></p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
            group(),
            x,
            y,
            cx,
            cy
        );
        zip.add(&format!("ppt/slides/slide{n}.xml"), slide.as_bytes(), true);
        zip.add(&format!("ppt/media/page{n}.png"), png, false);
        zip.add(
            &format!("ppt/slides/_rels/slide{n}.xml.rels"),
            rels(&[
                ("rId1", "image".into(), format!("../media/page{n}.png")),
                ("rId2", "slideLayout".into(), "../slideLayouts/slideLayout1.xml".into()),
            ])
            .as_bytes(),
            true,
        );
        types.push_str(&format!(
            "<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"
        ));
    }
    for (i, id) in slide_rel_ids.iter().enumerate() {
        presentation_rels.push((id.as_str(), "slide".into(), format!("slides/slide{}.xml", i + 1)));
    }
    types.push_str("</Types>");
    zip.add("[Content_Types].xml", types.as_bytes(), true);
    zip.add("_rels/.rels", rels(&[("rId1", "officeDocument".into(), "ppt/presentation.xml".into())]).as_bytes(), true);
    zip.add("ppt/presentation.xml",format!("<p:presentation xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{REL}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"rId1\"/></p:sldMasterIdLst><p:sldIdLst>{ids}</p:sldIdLst><p:sldSz cx=\"{sw:.0}\" cy=\"{sh:.0}\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>").as_bytes(),true);
    zip.add("ppt/_rels/presentation.xml.rels", rels(&presentation_rels).as_bytes(), true);
    zip.add("ppt/presProps.xml", format!("<p:presentationPr xmlns:p=\"{P}\"/>").as_bytes(), true);
    zip.add("ppt/slideMasters/slideMaster1.xml",format!("<p:sldMaster xmlns:p=\"{P}\" xmlns:a=\"{A}\" xmlns:r=\"{REL}\"><p:cSld><p:spTree>{}</p:spTree></p:cSld><p:clrMap accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" bg1=\"lt1\" bg2=\"lt2\" folHlink=\"folHlink\" hlink=\"hlink\" tx1=\"dk1\" tx2=\"dk2\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"rId1\"/></p:sldLayoutIdLst></p:sldMaster>",group()).as_bytes(),true);
    zip.add(
        "ppt/slideMasters/_rels/slideMaster1.xml.rels",
        rels(&[("rId1", "slideLayout".into(), "../slideLayouts/slideLayout1.xml".into()), ("rId2", "theme".into(), "../theme/theme1.xml".into())])
            .as_bytes(),
        true,
    );
    zip.add("ppt/slideLayouts/slideLayout1.xml",format!("<p:sldLayout xmlns:p=\"{P}\" xmlns:a=\"{A}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>",group()).as_bytes(),true);
    zip.add(
        "ppt/slideLayouts/_rels/slideLayout1.xml.rels",
        rels(&[("rId1", "slideMaster".into(), "../slideMasters/slideMaster1.xml".into())]).as_bytes(),
        true,
    );
    let mut colors = String::new();
    for (name, rgb) in [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "202020"),
        ("lt2", "F2F2F2"),
        ("accent1", "2868D9"),
        ("accent2", "D63883"),
        ("accent3", "289761"),
        ("accent4", "7550CB"),
        ("accent5", "D99026"),
        ("accent6", "228BA3"),
        ("hlink", "2868D9"),
        ("folHlink", "7550CB"),
    ] {
        colors.push_str(&format!("<a:{name}><a:srgbClr val=\"{rgb}\"/></a:{name}>"));
    }
    let fills = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>".repeat(3);
    let lines = "<a:ln w=\"6350\"><a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill><a:prstDash val=\"solid\"/></a:ln>".repeat(3);
    zip.add("ppt/theme/theme1.xml",format!("<a:theme xmlns:a=\"{A}\" name=\"MyAIPDF\"><a:themeElements><a:clrScheme name=\"MyAIPDF\">{colors}</a:clrScheme><a:fontScheme name=\"Inter\"><a:majorFont><a:latin typeface=\"Inter\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Inter\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Minimal\"><a:fillStyleLst>{fills}</a:fillStyleLst><a:lnStyleLst>{lines}</a:lnStyleLst><a:effectStyleLst><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle><a:effectStyle><a:effectLst/></a:effectStyle></a:effectStyleLst><a:bgFillStyleLst>{fills}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>").as_bytes(),true);
    Ok(zip.finish())
}
