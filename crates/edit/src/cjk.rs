//! Embedded Simplified Chinese CID TrueType subsets (ISO 32000-2 §9.7).
use crate::EditError;
use printcraft_cos::{Dict, Document, Object, PdfString, Stream};
use printcraft_fonts::SubsetGlyph;
use std::collections::BTreeMap;

pub(crate) fn is_chinese(text: &str) -> bool {
    text.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)) && !text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c))
}
#[derive(Clone)]
pub(crate) struct CjkFont {
    pub name: String,
    pub label: String,
    glyphs: BTreeMap<char, SubsetGlyph>,
}
impl CjkFont {
    pub fn encode(&self, text: &str) -> Option<Vec<u8>> {
        let mut out = Vec::with_capacity(text.len() * 2);
        for ch in text.chars() {
            out.extend_from_slice(&self.glyphs.get(&ch)?.cid.to_be_bytes());
        }
        Some(out)
    }
    pub fn width(&self, text: &str) -> f64 {
        text.chars().filter_map(|c| self.glyphs.get(&c)).map(|g| g.width / 1000.0).sum()
    }
    pub fn embed(doc: &mut Document, fonts: &mut Dict, text: &str, bold: bool, italic: bool) -> Result<Self, EditError> {
        let subset = printcraft_fonts::chinese_subset(text, bold).map_err(EditError::Invalid)?;
        let mut fd = Dict::new();
        fd.set(b"Length1".to_vec(), subset.bytes.len() as i64);
        let file = doc.add(Object::Stream(Stream::flate(fd, &subset.bytes)));
        let mut number = file.num;
        let tag: String = (0..6)
            .map(|_| {
                let c = char::from(b'A' + (number % 26) as u8);
                number /= 26;
                c
            })
            .collect();
        let base = format!("{tag}+{}", subset.name);
        let n = printcraft_content::num;
        let mut descriptor = Dict::new();
        descriptor.set(b"Type".to_vec(), Object::name("FontDescriptor"));
        descriptor.set(b"FontName".to_vec(), Object::name(&base));
        descriptor.set(b"Flags".to_vec(), if italic { 96i64 } else { 32i64 });
        descriptor.set(b"FontBBox".to_vec(), Object::Array(subset.bbox.into_iter().map(n).collect()));
        descriptor.set(b"Ascent".to_vec(), n(subset.ascent));
        descriptor.set(b"Descent".to_vec(), n(subset.descent));
        descriptor.set(b"CapHeight".to_vec(), n(subset.cap_height));
        descriptor.set(b"ItalicAngle".to_vec(), if italic { -12i64 } else { 0i64 });
        descriptor.set(b"StemV".to_vec(), if bold { 120i64 } else { 80i64 });
        descriptor.set(b"FontWeight".to_vec(), if bold { 600i64 } else { 400i64 });
        descriptor.set(b"FontFile2".to_vec(), Object::Ref(file));
        let descriptor = doc.add(Object::Dict(descriptor));
        let mut by_cid: Vec<_> = subset.glyphs.iter().collect();
        by_cid.sort_by_key(|(_, g)| g.cid);
        let mut gids = vec![0u8, 0];
        for (_, g) in &by_cid {
            gids.extend_from_slice(&g.gid.to_be_bytes());
        }
        let gid_map = doc.add(Object::Stream(Stream::flate(Dict::new(), &gids)));
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (MyAIPDF) /Ordering (Unicode) /Supplement 0 >> def\n/CMapName /MyAIPDF-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        for chunk in by_cid.chunks(100) {
            cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (ch, g) in chunk {
                let mut units = [0u16; 2];
                let unicode: String = ch.encode_utf16(&mut units).iter().map(|u| format!("{u:04X}")).collect();
                cmap.push_str(&format!("<{:04X}> <{unicode}>\n", g.cid));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let unicode = doc.add(Object::Stream(Stream::flate(Dict::new(), cmap.as_bytes())));
        let mut system = Dict::new();
        system.set(b"Registry".to_vec(), Object::String(PdfString::literal(b"Adobe".to_vec())));
        system.set(b"Ordering".to_vec(), Object::String(PdfString::literal(b"Identity".to_vec())));
        system.set(b"Supplement".to_vec(), 0i64);
        let mut cid = Dict::new();
        cid.set(b"Type".to_vec(), Object::name("Font"));
        cid.set(b"Subtype".to_vec(), Object::name("CIDFontType2"));
        cid.set(b"BaseFont".to_vec(), Object::name(&base));
        cid.set(b"CIDSystemInfo".to_vec(), Object::Dict(system));
        cid.set(b"FontDescriptor".to_vec(), Object::Ref(descriptor));
        cid.set(b"CIDToGIDMap".to_vec(), Object::Ref(gid_map));
        cid.set(b"W".to_vec(), Object::Array(vec![Object::Int(1), Object::Array(by_cid.iter().map(|(_, g)| n(g.width)).collect())]));
        let cid = doc.add(Object::Dict(cid));
        let mut font = Dict::new();
        font.set(b"Type".to_vec(), Object::name("Font"));
        font.set(b"Subtype".to_vec(), Object::name("Type0"));
        font.set(b"BaseFont".to_vec(), Object::name(&base));
        font.set(b"Encoding".to_vec(), Object::name("Identity-H"));
        font.set(b"DescendantFonts".to_vec(), Object::Array(vec![Object::Ref(cid)]));
        font.set(b"ToUnicode".to_vec(), Object::Ref(unicode));
        let mut suffix = 0;
        let mut name = "PCSC".to_string();
        while fonts.contains(name.as_bytes()) {
            suffix += 1;
            name = format!("PCSC{suffix}");
        }
        fonts.set(name.as_bytes().to_vec(), Object::Ref(doc.add(Object::Dict(font))));
        Ok(Self { name, label: format!("{} embedded subset", subset.name), glyphs: subset.glyphs })
    }
}
