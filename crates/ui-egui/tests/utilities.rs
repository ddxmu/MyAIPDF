use egui_kittest::{Harness, kittest::Queryable};
use printcraft_engine::catalog::{Availability, TOOL_GROUPS};
use printcraft_ui_egui::{Dialog, PrintCraftApp, QuickTool, UtilityKind};

const PDF:&[u8]=b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 400] >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";

fn harness() -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1380.0, 824.0)).build_eframe(|_| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("synthetic.pdf", None, PDF.to_vec()).unwrap();
        app.set_option("left", "closed").unwrap();
        app
    });
    h.run_steps(4);
    h
}

#[test]
fn utility_dialog_cancel_apply_save_and_undo() {
    let mut h = harness();
    let id = h.state().views[0].id;
    h.state_mut().execute("page.transitions");
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::Utility(UtilityKind::Transitions)));
    h.get_by_label("取消").hover();
    h.run_steps(2);
    h.get_by_label("取消").click();
    h.run_steps(3);
    assert!(!h.state().session.get(id).unwrap().dirty);
    h.state_mut().execute("page.transitions");
    h.run_steps(3);
    h.get_by_label("应用处理").hover();
    h.run_steps(2);
    h.get_by_label("应用处理").click();
    h.run_steps(3);
    let bytes = h.state_mut().session.save_bytes(id).unwrap();
    let cos = printcraft_cos::Document::open(bytes).unwrap();
    assert!(printcraft_model::pages(&cos)[0].dict.contains(b"Trans"));
    h.state_mut().execute("edit.undo");
    h.run_steps(2);
    let reverted = h.state_mut().session.save_bytes(id).unwrap();
    assert!(!printcraft_model::pages(&printcraft_cos::Document::open(reverted).unwrap())[0].dict.contains(b"Trans"));
    for command in ["prepress.convert_colors", "prepress.hairlines", "prepress.marks", "ocr.enhance", "measure.distance"] {
        assert!(h.state_mut().execute(command));
        h.run_steps(2);
        assert!(matches!(h.state().dialog, Some(Dialog::Utility(_))));
        h.state_mut().dialog = None;
    }
}

#[test]
fn measured_drag_adds_real_distance_annotation_and_is_undoable() {
    let mut h = harness();
    h.state_mut().execute("measure.distance");
    h.run_steps(3);
    h.get_by_label("页面拖拽测量").hover();
    h.run_steps(2);
    h.get_by_label("页面拖拽测量").click();
    h.run_steps(4);
    assert_eq!(h.state().quick_tool, QuickTool::Measure);
    let r = h.state().views[0].page_screen_rect(0).unwrap();
    let start = egui::pos2(r.left() + 30.0 / 300.0 * r.width(), r.top() + 30.0 / 400.0 * r.height());
    let end = egui::pos2(start.x + 72.0 / 300.0 * r.width(), start.y);
    h.hover_at(start);
    h.run_steps(1);
    h.drag_at(start);
    h.run_steps(1);
    for i in 1..=4 {
        h.hover_at(start + (end - start) * (i as f32 / 4.0));
        h.run_steps(1);
    }
    h.drop_at(end);
    h.run_steps(3);
    let id = h.state().views[0].id;
    let annotations = &h.state().session.get(id).unwrap().info.annotations;
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].subtype, "Line");
    assert!(annotations[0].contents.as_deref().unwrap_or_default().contains("25.400 mm"), "{annotations:?}");
    h.state_mut().execute("edit.undo");
    h.run_steps(2);
    assert!(h.state().session.get(id).unwrap().info.annotations.is_empty());
}

#[test]
fn working_catalog_items_are_ready_and_have_real_command_handlers() {
    let mut app = PrintCraftApp::new();
    app.open_bytes("synthetic.pdf", None, PDF.to_vec()).unwrap();
    for command in [
        "comment.cloud",
        "comment.stamp",
        "tools.js_console",
        "tools.document_js",
        "optimize.audit",
        "page.transitions",
        "measure.distance",
        "ocr.enhance",
    ] {
        let item = TOOL_GROUPS.iter().flat_map(|g| g.sections).flat_map(|s| s.items).find(|i| i.command == command).unwrap();
        assert_eq!(item.availability, Availability::Ready, "{command}");
        assert!(app.execute(command), "{command}");
        app.dialog = None;
    }
}
