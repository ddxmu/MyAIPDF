//! Export a PDF ▸ Image and Text from the real shell (egui_kittest).

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use printcraft_ui_egui::{Dialog, PrintCraftApp};

const FIXTURE: &[u8] = b"%PDF-1.7
1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj
2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 200 100] >> endobj
3 0 obj << /Type /Page /Parent 2 0 R >> endobj
4 0 obj << /Type /Page /Parent 2 0 R >> endobj
trailer << /Root 1 0 R >>
%%EOF";

#[test]
fn word_export_dialog_explains_both_modes_and_requires_explicit_export() {
    use printcraft_engine::compare::WordMode;
    let dir = std::env::temp_dir().join(format!(
        "printcraft-word-export-ui-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let target = dir.join("report.docx");
    let out = target.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("doc.pdf", None, FIXTURE.to_vec()).unwrap();
        app.save_override = Some(out.to_string_lossy().into_owned());
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("export.docx"));
    h.run_steps(2);
    assert_eq!(h.state().dialog, Some(Dialog::ExportWord));
    h.get_by_label("PDF 转 Word");
    h.get_by_label_contains("不能逐字编辑正文");
    assert_eq!(h.state().word_mode, WordMode::Preserve);
    assert!(!target.exists());
    h.get_by_label("取消").click();
    h.run_steps(2);
    assert!(!target.exists());
    h.state_mut().execute("export.docx");
    h.run_steps(2);
    h.get_by_label("可编辑文字（定位文本框）").click();
    h.run_steps(2);
    assert_eq!(h.state().word_mode, WordMode::Editable);
    h.get_by_label("导出 Word").click();
    h.run_steps(4);
    assert!(std::fs::read(&target).unwrap().starts_with(b"PK"));
    assert!(h.state().dialog.is_none());
    assert!(h.state_mut().set_option("word-mode", "unknown").is_err());
    std::fs::remove_file(&target).unwrap();
    std::fs::remove_dir(&dir).unwrap();
}

#[test]
fn export_dialogs_write_images_and_text() {
    let dir = std::env::temp_dir().join(format!("printcraft-export-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let d = dir.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("doc.pdf", None, FIXTURE.to_vec()).unwrap();
        app.export_dir_override = Some(d.to_string_lossy().into_owned());
        app
    });
    h.run_steps(4);
    assert!(h.state_mut().execute("export.image"));
    h.run_steps(2);
    h.get_by_label("Export to Image");
    h.get_by_label("Export").click();
    h.run_steps(4);
    assert!(dir.join("doc_page_1.png").exists() && dir.join("doc_page_2.png").exists());
    h.get_by_label_contains("Exported 2 images");
    assert!(h.state_mut().execute("export.text"));
    h.run_steps(2);
    h.get_by_label("Export").click();
    h.run_steps(4);
    assert!(dir.join("doc.txt").exists());
    // Export all images: this document has none, and says so.
    assert!(h.state_mut().execute("export.all_images"));
    h.run_steps(2);
    h.get_by_label("Export All Images");
    h.get_by_label("Export").click();
    h.run_steps(4);
    h.get_by_label_contains("Exported 0 images");
    h.state_mut().execute("export.ps");
    h.run_steps(2);
    h.get_by_label("PostScript / EPS 导出");
    h.get_by_label("Export").click();
    h.run_steps(4);
    assert!(std::fs::read(dir.join("doc.ps")).unwrap().starts_with(b"%!PS-Adobe-3.0\n"));
    h.state_mut().execute("export.ps");
    h.run_steps(2);
    h.get_by_label("EPS (.eps)").click();
    h.run_steps(2);
    h.get_by_label("Export").click();
    h.run_steps(4);
    for p in 1..=2 {
        let data = std::fs::read(dir.join(format!("doc_page_{p}.eps"))).unwrap();
        assert!(data.starts_with(b"%!PS-Adobe-3.0 EPSF-3.0"));
        assert!(data.windows(b"%%BoundingBox: 0 0 200 100".len()).any(|x| x == b"%%BoundingBox: 0 0 200 100"));
    }
}
