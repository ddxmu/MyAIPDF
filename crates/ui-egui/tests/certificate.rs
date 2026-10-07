use egui_kittest::{Harness, kittest::Queryable};
use printcraft_engine::sign::{Certificate, Time, pkcs12};
use printcraft_ui_egui::{Dialog, PrintCraftApp};

const PDF: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 300] >> endobj\n3 0 obj << /Type /Page /Parent 2 0 R >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF";

#[test]
fn certificate_dialog_saves_copy_and_opens_with_private_key() {
    let dir = std::env::temp_dir().join(format!("myaipdf-certificate-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let mut id = pkcs12::open(include_bytes!("../../sign/tests/data/rsa-aes.p12"), "test").unwrap();
    id.certificate = Certificate::self_signed_encryption(&id.certificate.subject, &id.key, Time::from_unix(now - 3600), 5, &[0x42]).unwrap();
    std::fs::write(dir.join("recipient.cer"), &id.certificate.raw).unwrap();
    std::fs::write(dir.join("identity.p12"), pkcs12::write(&id, "test").unwrap()).unwrap();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_eframe(|_| {
        let mut app = PrintCraftApp::new();
        app.open_bytes("source.pdf", None, PDF.to_vec()).unwrap();
        app
    });
    h.run_steps(4);
    h.state_mut().execute("protect.certificate");
    h.state_mut().certificate_draft.path = dir.join("recipient.cer").to_string_lossy().into_owned();
    h.state_mut().certificate_draft.output_override = Some(dir.join("encrypted.pdf").to_string_lossy().into_owned());
    h.run_steps(3);
    assert_eq!(h.state().dialog, Some(Dialog::CertificateProtect));
    assert!(h.query_by(|n| n.label().as_deref() == Some("另存加密 PDF") && n.is_disabled()).is_some());
    h.get_by_label("添加收件人").click();
    h.run_steps(3);
    assert_eq!(h.state().certificate_draft.recipients.len(), 1);
    h.get_by_label("已确认收件人持有对应私钥，保留原 PDF").click();
    h.run_steps(2);
    if let Ok(out) = std::env::var("MYAIPDF_UI_QA_DIR") {
        let language = h.state().language;
        h.state_mut().language = printcraft_ui_egui::i18n::Language::Zh;
        h.run_steps(3);
        h.render().unwrap().save(std::path::Path::new(&out).join("certificate-dialog.png")).unwrap();
        h.state_mut().language = language;
        h.run_steps(3);
    }
    h.get_by_label("另存加密 PDF").click();
    h.run_steps(3);
    assert_eq!(h.state().dialog, None);
    assert!(!h.state().session.get(h.state().views[0].id).unwrap().dirty);
    let cipher = std::fs::read(dir.join("encrypted.pdf")).unwrap();
    h.state_mut().open_bytes("encrypted.pdf", None, cipher).unwrap();
    h.run_steps(3);
    assert!(h.state().password_prompt.as_ref().unwrap().certificate);
    if let Ok(out) = std::env::var("MYAIPDF_UI_QA_DIR") {
        h.render().unwrap().save(std::path::Path::new(&out).join("certificate-open.png")).unwrap();
    }
    let private_path = dir.join("identity.p12").to_string_lossy().into_owned();
    h.get_by_label("私钥文件").click();
    h.run_steps(2);
    h.get_by_label("私钥文件").type_text(&private_path);
    h.run_steps(2);
    assert_eq!(h.state().password_prompt.as_ref().unwrap().identity_path, private_path, "password field must not steal path focus");
    h.state_mut().submit_password(Some("wrong".into()));
    assert!(h.state().password_prompt.as_ref().unwrap().error.is_some());
    h.state_mut().submit_password(Some("test".into()));
    h.run_steps(3);
    assert!(h.state().password_prompt.is_none());
    assert_eq!(h.state().views.len(), 2);
    let doc = h.state().session.get(h.state().views[1].id).unwrap();
    assert!(doc.info.encrypted && doc.uses_certificate_security());
    assert!(!doc.allows_modification());
    assert!(h.state().session.save_bytes(doc.id).unwrap().windows(b"/Adobe.PubSec".len()).any(|x| x == b"/Adobe.PubSec"));
}
