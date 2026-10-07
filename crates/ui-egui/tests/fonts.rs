//! The interface fonts with and without the optional craft-fonts build input (`CRAFT_FONTS_DIR`).

use egui::epaint::text::{Fonts, TextOptions};
use egui::{Color32, FontFamily, FontId};
use printcraft_ui_egui::theme;

const JAPANESE: &str = "日本語の文字";

fn families() -> Vec<FontId> {
    vec![FontId::proportional(13.0), FontId::monospace(13.0), theme::medium(13.0), theme::semibold(17.0)]
}

/// Lays `text` out in every interface family and returns each galley's width.
fn layout_widths(fonts: &mut Fonts, text: &str) -> Vec<f32> {
    let mut view = fonts.with_pixels_per_point(2.0);
    families().into_iter().map(|id| view.layout_no_wrap(text.to_owned(), id, Color32::BLACK).size().x).collect()
}

/// Built with craft-fonts, Japanese text renders with real glyphs (no tofu) in every family,
/// from a craft-fonts face placed after the app's own fonts.
#[test]
fn japanese_ui_text_uses_craft_fonts() {
    if printcraft_fonts::ui_japanese_fonts().is_empty() {
        eprintln!("skipping japanese_ui_text_uses_craft_fonts: built without craft-fonts (set CRAFT_FONTS_DIR to run it)");
        return;
    }
    let defs = theme::font_definitions();
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let stack = &defs.families[&family];
        let first_jp = stack.iter().position(|n| n.starts_with("BIZ UDPGothic")).expect("BIZ UDPGothic is a fallback");
        let own = stack.iter().position(|n| n == "Inter" || n == "JetBrainsMono").expect("the app's own font");
        assert!(own < first_jp, "{family:?}: {stack:?}");
        assert!(stack[first_jp..].iter().all(|n| printcraft_fonts::CRAFT_FONTS.iter().any(|f| f.name() == *n)), "{stack:?}");
    }
    let mut fonts = Fonts::new(TextOptions::default(), defs);
    for id in families() {
        assert!(fonts.has_glyphs(&id, JAPANESE), "{id:?} lacks {JAPANESE}");
    }
    // Real glyphs are about a full em wide each; tofu boxes and missing glyphs are not.
    for w in layout_widths(&mut fonts, JAPANESE) {
        assert!(w > 13.0 * 0.8 * JAPANESE.chars().count() as f32, "{w}");
    }
}

/// Without craft-fonts the interface fonts still install and lay out any text (Japanese falls
/// back to egui's replacement glyph) without panicking; Latin text is unaffected.
#[test]
fn ui_fonts_work_without_craft_fonts() {
    let mut fonts = Fonts::new(TextOptions::default(), theme::font_definitions());
    for id in families() {
        assert!(fonts.has_glyphs(&id, "PrintCraft"), "{id:?}");
        assert_eq!(fonts.has_glyphs(&id, JAPANESE), !printcraft_fonts::ui_japanese_fonts().is_empty(), "{id:?}");
    }
    assert!(layout_widths(&mut fonts, JAPANESE).iter().all(|w| w.is_finite() && *w > 0.0));
    assert!(layout_widths(&mut fonts, "PrintCraft").iter().all(|w| *w > 20.0));
    let ctx = egui::Context::default();
    theme::install_fonts(&ctx);
}

#[test]
fn chinese_ui_uses_weight_matched_sans_fonts() {
    if !printcraft_fonts::CRAFT_FONTS.iter().any(|f| f.family == "IBM Plex Sans SC") {
        eprintln!("skipping chinese_ui_uses_weight_matched_sans_fonts: build with MyAIPDF craft-fonts");
        return;
    }
    let defs = theme::font_definitions();
    for (family, style) in [
        (FontFamily::Proportional, "Regular"),
        (FontFamily::Monospace, "Regular"),
        (FontFamily::Name("medium".into()), "Medium"),
        (FontFamily::Name("semibold".into()), "SemiBold"),
    ] {
        let first_chinese =
            defs.families[&family].iter().find(|name| printcraft_fonts::CRAFT_FONTS.iter().any(|face| face.name() == **name && face.covers("Hans")));
        assert_eq!(first_chinese.map(String::as_str), Some(format!("IBM Plex Sans SC {style}").as_str()));
    }
    let mut fonts = Fonts::new(TextOptions::default(), defs);
    for id in families() {
        assert!(fonts.has_glyphs(&id, "黑体中文界面：文件、编辑、AI 助手、接口设置、拉取模型、确认执行、撤销。"), "{id:?}");
        let chinese: String = include_str!("../src/zh.rs")
            .chars()
            .chain(include_str!("../src/updates.rs").chars())
            .filter(|c| ('\u{3400}'..='\u{9fff}').contains(c))
            .collect();
        assert!(fonts.has_glyphs(&id, &chinese), "Chinese labels have missing glyphs in {id:?}");
    }
    let mut view = fonts.with_pixels_per_point(2.0);
    assert!(view.layout_no_wrap("中文黑体".into(), theme::regular(17.0), Color32::BLACK).size().y < 30.0);
}
