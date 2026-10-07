use egui_kittest::{Harness, kittest::Queryable};
use printcraft_engine::{Edit, Watermark};
use printcraft_ui_egui::{Dialog, LeftPanel, PrintCraftApp};

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";

fn harness() -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1380.0, 824.0)).build_eframe(|_| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("synthetic.pdf", None, PDF.to_vec()).unwrap();
        let id = app.views[0].id;
        app.session
            .apply(
                id,
                Edit::AddWatermark { pages: vec![0], settings: Watermark { text: "DRAFT".into(), ..Default::default() }, replace: false, file: None },
            )
            .unwrap();
        app.left = LeftPanel::Tool("watermark_remove");
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn watermark_return_arrow_preserves_document_and_analysis() {
    let mut h = harness();
    h.get_by_label("分析水印").click();
    h.run_steps(3);
    let id = h.state().views[0].id;
    let bytes = h.state().session.save_bytes(id).unwrap();
    let count = h.state().watermarks.candidates.len();
    if let Ok(out) = std::env::var("MYAIPDF_UI_QA_DIR") {
        h.render().unwrap().save(std::path::Path::new(&out).join("watermark-back-arrow.png")).unwrap();
    }
    h.get_by_label("返回工具").click();
    h.run_steps(3);
    assert_eq!(h.state().left, LeftPanel::AllTools);
    assert_eq!(h.state().watermarks.candidates.len(), count);
    assert_eq!(h.state().session.save_bytes(id).unwrap(), bytes);
}

#[test]
fn watermark_panel_analysis_selection_confirmation_and_undo() {
    let mut h = harness();
    h.get_by_label("分析水印").click();
    h.run_steps(3);
    assert!(!h.state().watermarks.candidates.is_empty());
    assert!(h.state().watermarks.selected.is_empty(), "never auto-select");
    h.get_by_label("已标记水印 · DRAFT").click();
    h.run_steps(2);
    h.get_by_label("预览位置").click();
    h.run_steps(2);
    assert!(h.state().views[0].flash.is_some());
    h.get_by_label("删除所选水印").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::RemoveWatermarks));
    h.get_by_label("取消").hover();
    h.run_steps(2);
    h.get_by_label("取消").click();
    h.run_steps(3);
    assert!(!h.state().watermarks.selected.is_empty());
    h.get_by_label("删除所选水印").click();
    h.run_steps(3);
    h.get_by_label("确认删除").hover();
    h.run_steps(2);
    h.get_by_label("确认删除").click();
    h.run_steps(3);
    let id = h.state().views[0].id;
    assert!(h.state().session.get(id).unwrap().dirty);
    assert!(h.state().watermarks.candidates.is_empty());
    h.state_mut().execute("edit.undo");
    h.state_mut().execute("watermark.analyze");
    h.run_steps(3);
    assert!(!h.state().watermarks.candidates.is_empty());
}

#[test]
fn analysis_becomes_stale_after_an_edit() {
    let mut h = harness();
    h.state_mut().execute("watermark.analyze");
    h.run_steps(3);
    h.get_by_label("已标记水印 · DRAFT").click();
    h.run_steps(2);
    h.state_mut().execute("page.rotate");
    h.run_steps(3);
    assert!(h.query_by(|n| n.label().as_deref() == Some("删除所选水印") && n.is_disabled()).is_some());
    h.get_by_label("文档已改变，请重新分析。旧选择不能删除。");
}
