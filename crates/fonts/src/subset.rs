//! Subset the approved Chinese TrueType build input for PDF CID font embedding.
use skrifa::instance::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};
use std::collections::BTreeMap;
use subsetter::GlyphRemapper;

/// UI-only derived face: use compact, shared baseline metrics without scaling any glyph.
/// Rename primary font names (OFL reserved name "Plex") and retain copyright/licence records.
/// Document embedding uses the unmodified build input, never this derived face.
pub fn chinese_ui_font(bytes: &[u8]) -> Option<Vec<u8>> {
    fn u16_at(b: &[u8], at: usize) -> Option<u16> {
        Some(u16::from_be_bytes(b.get(at..at + 2)?.try_into().ok()?))
    }
    fn u32_at(b: &[u8], at: usize) -> Option<u32> {
        Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
    }
    fn checksum(b: &[u8]) -> u32 {
        b.chunks(4).fold(0u32, |s, c| {
            let mut word = [0; 4];
            word[..c.len()].copy_from_slice(c);
            s.wrapping_add(u32::from_be_bytes(word))
        })
    }
    let count = usize::from(u16_at(bytes, 4)?);
    if count > 256 {
        return None;
    }
    let mut tables = Vec::new();
    for i in 0..count {
        let record = 12 + i * 16;
        let offset = usize::try_from(u32_at(bytes, record + 8)?).ok()?;
        let len = usize::try_from(u32_at(bytes, record + 12)?).ok()?;
        bytes.get(offset..offset.checked_add(len)?)?;
        tables.push((record, offset, len));
    }
    let table = |tag: &[u8]| tables.iter().find(|(r, _, _)| bytes.get(*r..*r + 4) == Some(tag)).copied();
    let (_, os2, os2_len) = table(b"OS/2")?;
    let (_, head, head_len) = table(b"head")?;
    let (_, name, name_len) = table(b"name")?;
    if os2_len < 78 || head_len < 54 || name_len < 6 {
        return None;
    }
    let em = f32::from(u16_at(bytes, head + 18)?);
    let mut out = bytes.to_vec();
    let selection = u16_at(bytes, os2 + 62)? | 0x80; // USE_TYPO_METRICS.
    out[os2 + 62..os2 + 64].copy_from_slice(&selection.to_be_bytes());
    for (at, value) in [(68, (em * 0.9).round() as i16), (70, -(em * 0.3).round() as i16), (72, 0)] {
        out[os2 + at..os2 + at + 2].copy_from_slice(&value.to_be_bytes());
    }
    let strings = name + usize::from(u16_at(bytes, name + 4)?);
    for i in 0..usize::from(u16_at(bytes, name + 2)?) {
        let record = name + 6 + i * 12;
        if record + 12 > name + name_len {
            return None;
        }
        if ![1, 3, 4, 6, 16, 17].contains(&u16_at(bytes, record + 6)?) {
            continue;
        }
        let start = strings + usize::from(u16_at(bytes, record + 10)?);
        let end = start + usize::from(u16_at(bytes, record + 8)?);
        if end > name + name_len {
            return None;
        }
        let s = out.get_mut(start..end)?;
        for (old, new) in [(b"Plex".as_slice(), b"MyAI".as_slice()), (b"\0P\0l\0e\0x".as_slice(), b"\0M\0y\0A\0I".as_slice())] {
            for at in 0..s.len().saturating_sub(old.len()).saturating_add(1) {
                if s.get(at..at + old.len()) == Some(old) {
                    s[at..at + new.len()].copy_from_slice(new);
                }
            }
        }
    }
    out[head + 8..head + 12].fill(0);
    for (record, offset, len) in tables {
        let sum = checksum(&out[offset..offset + len]);
        out[record + 4..record + 8].copy_from_slice(&sum.to_be_bytes());
    }
    let adjustment = 0xb1b0_afbau32.wrapping_sub(checksum(&out));
    out[head + 8..head + 12].copy_from_slice(&adjustment.to_be_bytes());
    Some(out)
}

#[derive(Clone)]
pub struct SubsetGlyph {
    pub cid: u16,
    pub gid: u16,
    /// Advance in PDF glyph units (1000 per em).
    pub width: f64,
}
pub struct FontSubset {
    pub bytes: Vec<u8>,
    pub glyphs: BTreeMap<char, SubsetGlyph>,
    pub name: String,
    pub ascent: f64,
    pub descent: f64,
    pub cap_height: f64,
    pub bbox: [f64; 4],
}

/// Missing characters fail explicitly, never become `?`. No system font is read.
pub fn chinese_subset(text: &str, bold: bool) -> Result<FontSubset, String> {
    let style = if bold { "SemiBold" } else { "Regular" };
    let face = crate::CRAFT_FONTS
        .iter()
        .find(|f| f.covers("Hans") && f.style == style)
        .ok_or_else(|| "this build has no Chinese fallback font; build with MyAIPDF CRAFT_FONTS_DIR".to_string())?;
    let font = FontRef::new(face.bytes).map_err(|e| format!("Chinese font is invalid: {e}"))?;
    let loc = LocationRef::default();
    let metrics = font.metrics(Size::unscaled(), loc);
    let units = 1000.0 / f64::from(metrics.units_per_em.max(1));
    let charmap = font.charmap();
    let advances = font.glyph_metrics(Size::unscaled(), loc);
    let mut remapper = GlyphRemapper::new();
    let mut glyphs = BTreeMap::new();
    for ch in text.chars().filter(|ch| !matches!(ch, '\n' | '\r')) {
        if glyphs.contains_key(&ch) {
            continue;
        }
        let cid = u16::try_from(glyphs.len() + 1).map_err(|_| "Chinese paragraph has too many unique characters".to_string())?;
        let gid =
            charmap.map(ch).filter(|gid| gid.to_u32() != 0).ok_or_else(|| format!("Chinese fallback font has no glyph for U+{:04X}", ch as u32))?;
        let old_gid = u16::try_from(gid.to_u32()).map_err(|_| "Chinese glyph ID is too large".to_string())?;
        let width = f64::from(advances.advance_width(gid).unwrap_or(0.0)) * units;
        glyphs.insert(ch, SubsetGlyph { cid, gid: remapper.remap(old_gid), width });
    }
    let bytes = subsetter::subset(face.bytes, 0, &remapper).map_err(|e| format!("Chinese font subsetting failed: {e}"))?;
    let bbox = metrics.bounds.map_or([-1000.0, -300.0, 2000.0, 1200.0], |b| [b.x_min, b.y_min, b.x_max, b.y_max].map(|v| f64::from(v) * units));
    Ok(FontSubset {
        bytes,
        glyphs,
        name: format!("IBMPlexSansSC-{style}"),
        ascent: f64::from(metrics.ascent) * units,
        descent: f64::from(metrics.descent) * units,
        cap_height: f64::from(metrics.cap_height.unwrap_or(metrics.ascent)) * units,
        bbox,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_metrics_are_compact_without_resizing_glyphs() {
        for face in crate::CRAFT_FONTS.iter().filter(|f| f.covers("Hans")) {
            let bytes = chinese_ui_font(face.bytes).expect("approved face can be normalized");
            let original = FontRef::new(face.bytes).unwrap();
            let derived = FontRef::new(&bytes).unwrap();
            use skrifa::string::StringId;
            let name = |font: &FontRef<'_>, id| font.localized_strings(id).english_or_first().map(|s| s.chars().collect::<String>());
            assert_eq!(name(&derived, StringId::COPYRIGHT_NOTICE), name(&original, StringId::COPYRIGHT_NOTICE));
            assert!(!name(&derived, StringId::FAMILY_NAME).unwrap().contains("Plex"));
            let m = derived.metrics(Size::unscaled(), LocationRef::default());
            assert!((m.ascent - m.descent + m.leading) / f32::from(m.units_per_em) < 1.21);
            for ch in "黑体中文MyAIPDF0123".chars() {
                assert_eq!(derived.charmap().map(ch), original.charmap().map(ch));
            }
            let sum = bytes.chunks(4).fold(0u32, |s, c| {
                let mut b = [0; 4];
                b[..c.len()].copy_from_slice(c);
                s.wrapping_add(u32::from_be_bytes(b))
            });
            assert_eq!(sum, 0xb1b0_afba);
        }
        assert!(chinese_ui_font(&[]).is_none());
        assert!(chinese_ui_font(&[0; 30]).is_none());
    }
}
