//! Screenshots using only synthetic, contributor-original content and approved UI fonts.
//! Usage: myaipdf_shot <output.png> <watermark|ai|settings|settings-dark|settings-compact>
use egui_kittest::Harness;
use printcraft_engine::{Edit, Watermark};
use printcraft_ui_egui::{LeftPanel, PrintCraftApp};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let output = args.first().ok_or("missing screenshot output")?;
    let scene = args.get(1).cloned().unwrap_or_else(|| "watermark".into());
    let watermark = scene == "watermark";
    let size = if scene == "settings-compact" { egui::vec2(1024.0, 700.0) } else { egui::vec2(1380.0, 824.0) };
    let mut h = Harness::builder().with_size(size).with_pixels_per_point(2.0).build_eframe(move |_| {
        let mut app = PrintCraftApp::new();
        let _ = app.set_option("language", "zh");
        if scene == "settings-dark" {
            let _ = app.set_option("theme", "dark");
        }
        if scene == "watermark" {
            let bytes = app.session.create_from_text(
                "原创去水印测试文档",
                "Watermark removal safety test\n\nThis is contributor-original MyAIPDF content.\nKeep this paragraph; remove only the selected watermark.\nUndo is available. Save the result as a separate PDF.",
            );
            if let Ok(bytes) = bytes
                && app.open_bytes("原创水印验证.pdf", None, bytes.as_ref().clone()).is_ok()
                && let Some(view) = app.views.first()
            {
                let _ = app.session.apply(
                    view.id,
                    Edit::AddWatermark {
                        pages: vec![0],
                        settings: Watermark { text: "DRAFT".into(), opacity: 0.2, ..Default::default() },
                        replace: false,
                        file: None,
                    },
                );
            }
            if let Some(view) = app.views.first_mut() {
                view.fit = printcraft_ui_egui::canvas::Fit::Page;
            }
            app.left = LeftPanel::Tool("watermark_remove");
            app.execute("watermark.analyze");
        } else {
            app.left = LeftPanel::Tool("ai");
            app.ai.settings_open = scene.starts_with("settings");
        }
        app
    });
    for _ in 0..200 {
        h.run_steps(2);
        if !h.state().render_pending() {
            h.run_steps(4);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    if watermark && h.state().watermarks.candidates.is_empty() {
        return Err("synthetic watermark analysis failed".into());
    }
    h.render()?.save(output).map_err(|e| e.to_string())
}
