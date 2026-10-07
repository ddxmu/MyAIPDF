//! Private compatibility check: actual UI hit, pending draft, save and reopen. No screenshot,
//! no preferences loaded, no original file overwritten. Usage: verify_edit input.pdf output.pdf.
use egui_kittest::Harness;
use printcraft_ui_egui::{PrintCraftApp, SaveTarget};

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let input = args.first().ok_or("input.pdf required")?;
    let output = args.get(1).ok_or("output.pdf required")?.clone();
    if std::path::Path::new(&output).exists() {
        return Err("output must not already exist".into());
    }
    let bytes = std::fs::read(input).map_err(|e| e.to_string())?;
    let original = bytes.clone();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(move |_cc| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("private-compatibility.pdf", None, bytes).unwrap();
        app.save_override = Some(output.clone());
        app
    });
    h.run_steps(4);
    h.state_mut().execute("edit.edit_text");
    h.run_steps(4);
    let app = h.state();
    let doc = app.session.get(app.views[0].id).ok_or("no document")?;
    let pages = doc.info.pages.len();
    let counts: Vec<_> = (0..pages).map(|p| doc.text_blocks(p).len()).collect();
    let blocks = doc.text_blocks(0);
    let (index, block) = blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| b.text.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c)))
        .max_by(|(_, a), (_, b)| a.size.total_cmp(&b.size))
        .ok_or("no Chinese text block")?;
    let block = block.clone();
    let page = &doc.info.pages[0];
    let screen = app.views[0].page_screen_rect(0).ok_or("page not visible")?;
    let x = (block.rect[0] + block.rect[2]) as f32 / 2.0;
    let y = (block.rect[1] + block.rect[3]) as f32 / 2.0;
    let at = egui::pos2(
        screen.left() + x / page.width as f32 * screen.width(),
        screen.top() + (page.height as f32 - y) / page.height as f32 * screen.height(),
    );
    h.hover_at(at);
    h.run_steps(1);
    h.drag_at(at);
    h.run_steps(1);
    h.drop_at(at);
    h.run_steps(3);
    let editor = h.state_mut().views[0].line_editor.as_mut().ok_or("click did not open editor")?;
    if editor.block != index {
        return Err("click selected a different block".into());
    }
    let expected = format!("{}【编辑验证】", block.text);
    editor.text = expected.clone();
    editor.look.size = block.size + 1.0;
    if !h.state_mut().save_active(SaveTarget::InPlace) {
        return Err("save failed".into());
    }
    let doc = h.state().session.get(h.state().views[0].id).ok_or("saved document closed")?;
    if doc.dirty || !doc.text_blocks(0).iter().any(|b| b.text == expected) {
        return Err("reopened text differs".into());
    }
    if std::fs::read(input).map_err(|e| e.to_string())? != original {
        return Err("original changed".into());
    }
    println!("pages={pages}, text_blocks_per_page={counts:?}; UI click, draft save, reopen, original unchanged: PASS");
    Ok(())
}
