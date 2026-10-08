//! Fixed-page Word export: no guessed tables or flowing paragraphs. Artwork stays at
//! its PDF position; optional editable text boxes explicitly name the source fonts.

use crate::{Zip, esc};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Page images, not editable text.
    #[default]
    Preserve,
    /// Positioned text boxes; fonts must be available in Word.
    Editable,
}

impl Mode {
    pub fn id(self) -> &'static str {
        match self {
            Self::Preserve => "preserve",
            Self::Editable => "editable",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "preserve" => Some(Self::Preserve),
            "editable" => Some(Self::Editable),
            _ => None,
        }
    }
}

pub struct Text {
    pub text: String,
    /// Display space, top-left origin, points.
    pub rect: [f64; 4],
    pub font: String,
    pub size: f64,
    pub bold: bool,
    pub italic: bool,
    pub color: [f64; 3],
}

pub struct Page {
    pub width: f64,
    pub height: f64,
    /// Full PDF page, or artwork after the editable text was made invisible.
    pub png: Vec<u8>,
    pub text: Vec<Text>,
}

const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";

fn family(name: &str) -> String {
    let name = name.split_once('+').filter(|(tag, _)| tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase())).map_or(name, |(_, n)| n);
    let lower = name.to_ascii_lowercase();
    let family = if lower.starts_with("microsoftyahei") {
        "Microsoft YaHei"
    } else if lower.starts_with("simsun") {
        "SimSun"
    } else if lower.starts_with("simhei") {
        "SimHei"
    } else if lower.starts_with("arialuni") {
        "Arial Unicode MS"
    } else if lower.starts_with("timesnewroman") || lower.starts_with("times-") {
        "Times New Roman"
    } else if lower.starts_with("helvetica") || lower.starts_with("arial") {
        "Arial"
    } else if lower.starts_with("courier") {
        "Courier New"
    } else {
        name.strip_suffix("-BoldItalic")
            .or_else(|| name.strip_suffix("-Bold"))
            .or_else(|| name.strip_suffix("-Italic"))
            .or_else(|| name.strip_suffix("-Regular"))
            .unwrap_or(name)
    };
    if family.is_empty() { "Arial".into() } else { family.into() }
}

fn section(p: &Page) -> String {
    format!(
        "<w:sectPr><w:type w:val=\"nextPage\"/><w:pgSz w:w=\"{:.0}\" w:h=\"{:.0}\"/><w:pgMar w:top=\"0\" w:right=\"0\" w:bottom=\"0\" w:left=\"0\" w:header=\"0\" w:footer=\"0\" w:gutter=\"0\"/></w:sectPr>",
        p.width * 20.0,
        p.height * 20.0
    )
}

fn textbox(t: &Text, id: usize) -> Result<String, String> {
    if t.rect.iter().chain([&t.size]).chain(t.color.iter()).any(|v| !v.is_finite())
        || t.size <= 0.0
        || t.rect[2] <= t.rect[0]
        || t.rect[3] <= t.rect[1]
    {
        return Err("文字位置或字号无效，请使用原样保真导出".into());
    }
    let font = esc(&family(&t.font));
    let size = (t.size * 2.0).round().clamp(2.0, 3276.0);
    let color: String = t.color.iter().map(|v| format!("{:02X}", (v.clamp(0.0, 1.0) * 255.0).round() as u8)).collect();
    let width = t.rect[2] - t.rect[0];
    // Fit the run to its measured width, so fallback metrics cannot wrap it into another line.
    let text_width = (width * 20.0).round().clamp(1.0, 31680.0);
    let height = (t.rect[3] - t.rect[1]).max(t.size * 1.5);
    Ok(format!(
        "<w:r><w:pict><v:rect id=\"text{id}\" o:spid=\"_x0000_s{}\" style=\"position:absolute;margin-left:{:.3}pt;margin-top:{:.3}pt;width:{:.3}pt;height:{height:.3}pt;z-index:1;mso-position-horizontal-relative:page;mso-position-vertical-relative:page;mso-wrap-style:none\" filled=\"f\" stroked=\"f\"><v:textbox inset=\"0,0,0,0\"><w:txbxContent><w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"{:.0}\" w:lineRule=\"exact\"/><w:jc w:val=\"left\"/></w:pPr><w:r><w:rPr><w:rFonts w:ascii=\"{font}\" w:hAnsi=\"{font}\" w:eastAsia=\"{font}\" w:cs=\"{font}\"/>{}{}<w:color w:val=\"{color}\"/><w:sz w:val=\"{size:.0}\"/><w:szCs w:val=\"{size:.0}\"/><w:fitText w:val=\"{text_width:.0}\" w:id=\"{id}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r></w:p></w:txbxContent></v:textbox><w10:wrap type=\"none\"/></v:rect></w:pict></w:r>",
        id + 1024,
        t.rect[0],
        t.rect[1],
        width + 0.5,
        t.size * 24.0,
        if t.bold { "<w:b/><w:bCs/>" } else { "" },
        if t.italic { "<w:i/><w:iCs/>" } else { "" },
        esc(&t.text)
    ))
}

pub fn docx(pages: &[Page], title: &str, mode: Mode) -> Result<Vec<u8>, String> {
    if pages.is_empty() || pages.len() > 500 {
        return Err("Word 导出支持 1 至 500 页，请分批导出".into());
    }
    let mut zip = Zip::default();
    let mut xml = String::from(
        "<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:v=\"urn:schemas-microsoft-com:vml\" xmlns:o=\"urn:schemas-microsoft-com:office:office\" xmlns:w10=\"urn:schemas-microsoft-com:office:word\"><w:body>",
    );
    let mut relationships =
        format!("<Relationships xmlns=\"{RELS}\"><Relationship Id=\"settings\" Type=\"{REL}/settings\" Target=\"settings.xml\"/>");
    let (mut bytes, mut count, mut id) = (0usize, 0usize, 1usize);
    for (i, p) in pages.iter().enumerate() {
        if !p.width.is_finite() || !p.height.is_finite() || !(7.2..=1584.0).contains(&p.width) || !(7.2..=1584.0).contains(&p.height) {
            return Err(format!("第 {} 页超出 Word 的页面尺寸限制（0.1 至 22 英寸），未缩放原稿", i + 1));
        }
        if !p.png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(format!("第 {} 页图像无效", i + 1));
        }
        bytes = bytes.saturating_add(p.png.len());
        count = count.saturating_add(p.text.len());
        if bytes > 256 * 1024 * 1024 || count > 100_000 {
            return Err("Word 导出内容过大，请分批导出".into());
        }
        let n = i + 1;
        let desc = match mode {
            Mode::Preserve => "PDF 原样保真页面图片，正文不可逐字编辑",
            Mode::Editable => "PDF 页面背景；可编辑文字在定位文本框内，其他内容保持图像",
        };
        xml.push_str("<w:p><w:pPr><w:spacing w:before=\"0\" w:after=\"0\" w:line=\"20\" w:lineRule=\"exact\"/>");
        if n < pages.len() {
            xml.push_str(&section(p));
        }
        xml.push_str("</w:pPr>");
        xml.push_str(&format!("<w:r><w:pict><v:rect id=\"page{n}\" o:spid=\"_x0000_s{}\" style=\"position:absolute;margin-left:0pt;margin-top:0pt;width:{:.3}pt;height:{:.3}pt;z-index:-1;mso-position-horizontal-relative:page;mso-position-vertical-relative:page\" stroked=\"f\"><v:imagedata r:id=\"image{n}\" o:title=\"{desc}\"/><w10:wrap type=\"none\"/></v:rect></w:pict></w:r>", n + 1_000_000, p.width, p.height));
        if mode == Mode::Editable {
            for t in &p.text {
                xml.push_str(&textbox(t, id)?);
                id += 1;
                if xml.len() > 64 * 1024 * 1024 {
                    return Err("Word 文字内容过大，请分批导出".into());
                }
            }
        }
        xml.push_str("</w:p>");
        zip.add(&format!("word/media/image{n}.png"), &p.png, false);
        relationships.push_str(&format!("<Relationship Id=\"image{n}\" Type=\"{REL}/image\" Target=\"media/image{n}.png\"/>"));
    }
    if let Some(last) = pages.last() {
        xml.push_str(&section(last));
    }
    xml.push_str("</w:body></w:document>");
    relationships.push_str("</Relationships>");
    zip.add("word/document.xml", xml.as_bytes(), true);
    zip.add("word/_rels/document.xml.rels", relationships.as_bytes(), true);
    zip.add("word/settings.xml", b"<w:settings xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:doNotAutoCompressPictures/><w:compat><w:compatSetting w:name=\"compatibilityMode\" w:uri=\"http://schemas.microsoft.com/office/word\" w:val=\"15\"/></w:compat></w:settings>", true);
    zip.add("[Content_Types].xml", format!("<Types xmlns=\"{CT}\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/settings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml\"/><Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/></Types>").as_bytes(), true);
    zip.add("_rels/.rels", format!("<Relationships xmlns=\"{RELS}\"><Relationship Id=\"doc\" Type=\"{REL}/officeDocument\" Target=\"word/document.xml\"/><Relationship Id=\"props\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/></Relationships>").as_bytes(), true);
    zip.add("docProps/core.xml", format!("<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:title>{}</dc:title><dc:description>{}</dc:description></cp:coreProperties>", esc(title), if mode == Mode::Preserve { "原样保真：页面图片，非可编辑正文" } else { "定位文本框：需安装原字体；背景、旋转文字与未识别内容为图像，非原生 Word 表格" }).as_bytes(), true);
    Ok(zip.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::part;
    fn page() -> Page {
        Page {
            width: 595.3,
            height: 841.9,
            png: b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            text: vec![Text {
                text: "报告 <&> 2024\u{0}".into(),
                rect: [72.0, 200.0, 372.0, 216.0],
                font: "ABCDEF+MicrosoftYaHei-Bold".into(),
                size: 16.0,
                bold: true,
                italic: false,
                color: [0.0, 0.0, 0.0],
            }],
        }
    }
    #[test]
    fn fixed_pages_preserve_images_and_mixed_sizes_without_flowing_tables() {
        let mut second = page();
        second.width = 841.9;
        second.height = 595.3;
        let zip = docx(&[page(), second], "Report", Mode::Preserve).unwrap();
        let xml = part(&zip, "word/document.xml");
        assert_eq!(xml.matches("<v:imagedata").count(), 2);
        assert_eq!(xml.matches("<w:sectPr>").count(), 2);
        assert!(xml.contains("w:w=\"16838\" w:h=\"11906\""));
        assert!(!xml.contains("<w:tbl") && !xml.contains("<w:txbxContent"));
        assert!(part(&zip, "docProps/core.xml").contains("非可编辑正文"));
    }
    #[test]
    fn editable_runs_keep_named_cjk_fonts_size_color_and_page_position() {
        let zip = docx(&[page()], "Report", Mode::Editable).unwrap();
        let xml = part(&zip, "word/document.xml");
        assert!(xml.contains("w:eastAsia=\"Microsoft YaHei\""));
        assert!(xml.contains("w:sz w:val=\"32\""));
        assert!(xml.contains("margin-left:72.000pt;margin-top:200.000pt"));
        assert!(xml.contains("报告 &lt;&amp;&gt; 2024") && !xml.contains('\u{0}'));
        assert!(xml.contains("<w:fitText") && !xml.contains("<w:tbl"));
        assert!(docx(&[], "Empty", Mode::Preserve).is_err());
        let mut p = page();
        p.width = 2000.0;
        assert!(docx(&[p], "Too large", Mode::Preserve).is_err());
        let mut p = page();
        p.text[0].rect[0] = f64::NAN;
        assert!(docx(&[p], "Invalid", Mode::Editable).is_err());
    }
}
