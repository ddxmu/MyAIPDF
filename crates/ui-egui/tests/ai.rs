//! Real UI buttons → compatible API → proposal → confirmed, undoable PDF edits.
use egui_kittest::{Harness, kittest::Queryable};
use printcraft_ai::{Action, Provider};
use printcraft_ui_egui::{LeftPanel, PrintCraftApp, ai_ui::Pending, i18n::Language};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] /Contents 5 0 R /Resources << /Font << /F1 6 0 R >> >> >> endobj\n4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 300 400] >> endobj\n5 0 obj << /Length 43 >> stream\nBT /F1 18 Tf 30 200 Td (PRIVATE PDF TEXT) Tj ET\nendstream endobj\n6 0 obj << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";

fn harness(provider: Provider) -> Harness<'static, PrintCraftApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 1400.0)).build_eframe(move |_| {
        let mut app = PrintCraftApp::new();
        app.language = Language::Zh;
        app.open_bytes("AI-test.pdf", None, PDF.to_vec()).unwrap();
        app.ai.preferences.providers[0] = provider;
        app.left = LeftPanel::Tool("ai");
        app
    });
    h.run_steps(4);
    h
}

fn wait(h: &mut Harness<'static, PrintCraftApp>) {
    let until = Instant::now() + Duration::from_secs(15);
    while h.state().ai.busy() {
        assert!(Instant::now() < until, "AI worker timed out");
        std::thread::sleep(Duration::from_millis(10));
        h.run_steps(2);
    }
    h.run_steps(3);
}

#[test]
fn chinese_ui_models_chat_confirm_undo_and_privacy() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let log = received.clone();
    let server = std::thread::spawn(move || {
        for i in 0..2 {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut data = Vec::new();
            let mut b = [0u8; 2048];
            loop {
                let n = stream.read(&mut b).unwrap();
                assert!(n > 0);
                data.extend_from_slice(&b[..n]);
                if let Some(end) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..end]);
                    let length = headers
                        .lines()
                        .find_map(|s| s.to_lowercase().strip_prefix("content-length:").and_then(|s| s.trim().parse::<usize>().ok()))
                        .unwrap_or(0);
                    if data.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let req = String::from_utf8(data).unwrap();
            if i == 0 {
                assert!(req.starts_with("GET /v1/models"));
            } else {
                assert!(req.starts_with("POST /v1/chat/completions"));
            }
            log.lock().unwrap().push(req);
            let body = if i == 0 { json!({"data":[{"id":"test-chat-model"}]}) }
                else { json!({"choices":[{"message":{"content":json!({"reply":"请确认将第 1 页顺时针旋转 90 度。","actions":[{"tool":"page_rotate","args":{"pages":[1],"degrees":90}}]}).to_string()}}]}) }.to_string();
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                .unwrap();
        }
    });
    let mut h = harness(Provider { base_url: format!("http://{address}/v1"), api_key: "dummy-test-key".into(), ..Default::default() });
    h.get_by_label("接口设置").click();
    h.run_steps(3);
    h.get_by_label("拉取模型").hover();
    h.run_steps(2);
    h.get_by_label("拉取模型").click();
    h.run_steps(2);
    wait(&mut h);
    assert_eq!(h.state().ai.preferences.providers[0].model, "test-chat-model");
    h.get_by_label("完成设置").click();
    h.run_steps(3);
    h.state_mut().ai.input = "将当前页顺时针旋转 90 度".into();
    h.run_steps(3);
    h.get_by_label("发送给 AI").click();
    h.run_steps(2);
    wait(&mut h);
    let id = h.state().views[0].id;
    assert_eq!(h.state().session.get(id).unwrap().info.pages[0].rotation, 0);
    assert!(!h.state().session.get(id).unwrap().dirty, "proposal must not edit");
    assert!(h.state().ai.pending.is_some());
    h.get_by_label("确认执行（不保存）").click();
    h.run_steps(4);
    assert_eq!(h.state().session.get(id).unwrap().info.pages[0].rotation, 90);
    assert!(h.state().session.get(id).unwrap().dirty);
    assert!(h.state_mut().execute("edit.undo"));
    h.run_steps(3);
    assert_eq!(h.state().session.get(id).unwrap().info.pages[0].rotation, 0);
    server.join().unwrap();
    let requests = received.lock().unwrap();
    assert!(!requests[1].contains("PRIVATE PDF TEXT"));
    let body: Value = serde_json::from_str(requests[1].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(body["model"], "test-chat-model");
    assert!(!h.state().persist().contains("dummy-test-key"));
}

#[test]
fn stale_plan_and_cross_document_plan_cannot_edit() {
    let mut h = harness(Provider::default());
    let id = h.state().views[0].id;
    let source = h.state().session.get(id).unwrap().export_source();
    let actions = vec![Action { tool: "page_rotate".into(), args: json!({"pages":[1],"degrees":90}) }];
    h.state_mut().ai.pending = Some(Pending { doc: id, bytes: source.bytes.clone(), actions: actions.clone() });
    h.state_mut().execute("page.rotate");
    h.state_mut().confirm_ai_plan();
    assert_eq!(h.state().session.get(id).unwrap().info.pages[0].rotation, 90, "stale proposal must not rotate twice");
    assert!(h.state().ai.status.contains("版本已变化"));
    h.state_mut().ai.pending = Some(Pending { doc: id, bytes: h.state().session.get(id).unwrap().export_source().bytes, actions });
    h.state_mut().open_bytes("other.pdf", None, PDF.to_vec()).unwrap();
    h.state_mut().confirm_ai_plan();
    let other = h.state().views.last().unwrap().id;
    assert_eq!(h.state().session.get(other).unwrap().info.pages[0].rotation, 0);
    assert!(!h.state().session.get(other).unwrap().dirty);
}

#[test]
fn settings_are_a_separate_modal_and_leave_the_chat_composer_visible() {
    let mut h = harness(Provider { id: "ui-only-fixture".into(), api_key: "private-ui-fixture".into(), ..Default::default() });
    assert!(h.query_by_label("AI 接口设置").is_none());
    h.get_by_label("问题或 PDF 处理要求");
    h.get_by_label("接口设置").click();
    h.run_steps(3);
    h.get_by_label("AI 接口设置");
    h.get_by_label("API 地址（OpenAI 兼容）");
    h.get_by_label("新增接口").hover();
    h.run_steps(2);
    h.get_by_label("新增接口").click();
    h.run_steps(3);
    assert_eq!(h.state().ai.preferences.providers.len(), 2);
    assert!(h.state().ai.preferences.providers[1].base_url.is_empty());
    h.get_by_label("显示").click();
    h.run_steps(2);
    assert!(h.state().ai.show_key);
    h.get_by_label("完成设置").click();
    h.run_steps(3);
    assert!(!h.state().ai.settings_open && !h.state().ai.show_key);
    assert!(!h.state().persist().contains("private-ui-fixture"));
    h.get_by_label("问题或 PDF 处理要求");
}

#[test]
fn settings_actions_remain_visible_in_small_windows() {
    for size in [egui::vec2(1024.0, 700.0), egui::vec2(800.0, 600.0)] {
        let mut h = Harness::builder().with_size(size).build_eframe(|_| {
            let mut app = PrintCraftApp::new();
            app.language = Language::Zh;
            app.ai.settings_open = true;
            app
        });
        h.run_steps(4);
        for label in ["保存接口设置", "忘记已存密钥", "完成设置"] {
            let rect = h.get_by_label(label).rect();
            assert!(rect.min.y >= 0.0 && rect.max.y <= size.y && rect.min.x >= 0.0 && rect.max.x <= size.x, "{label}: {rect:?}");
        }
    }
}

#[test]
fn only_known_qa_settings_are_removed_not_real_local_or_cloud_providers() {
    let mut app = PrintCraftApp::new();
    app.restore(
        &json!({"ai":{"selected":1,"providers":[
            {"id":"old-qa","base_url":"http://127.0.0.1:18473/v1","model":"qa-secondary-model","models":["qa-chat-model","qa-secondary-model"]},
            {"id":"real-local","base_url":"http://127.0.0.1:18473/v1","model":"real-model","models":["real-model"]},
            {"id":"cloud","base_url":"https://example.invalid/v1","model":"cloud-chat"}
        ]}})
        .to_string(),
    );
    let p = &app.ai.preferences.providers;
    assert!(p[0].base_url.is_empty() && p[0].models.is_empty() && p[0].model.is_empty());
    assert_eq!(p[1].model, "real-model");
    assert_eq!(p[1].base_url, "http://127.0.0.1:18473/v1");
    assert_eq!(p[2].base_url, "https://example.invalid/v1");
    assert_eq!(app.ai.preferences.selected, 1);
    assert!(!app.ai.busy());
}
