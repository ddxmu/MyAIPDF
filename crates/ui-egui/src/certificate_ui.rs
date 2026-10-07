//! Certificate security always writes an independent copy, retaining the original document.

use crate::{
    PrintCraftApp,
    theme::{self, Tokens},
    widgets,
};
use printcraft_engine::sign::Certificate;

#[derive(Default)]
pub struct CertificateDraft {
    pub path: String,
    pub recipients: Vec<Certificate>,
    pub full_control: bool,
    pub acknowledged: bool,
    pub error: Option<String>,
    /// UI test / opt-in automation output; never persisted.
    pub output_override: Option<String>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_bounded(path: &str, max: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(max.saturating_add(1)).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("证书文件过大".into());
    }
    Ok(bytes)
}

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PrintCraftApp, t: &Tokens) -> bool {
    ui.label(egui::RichText::new("使用证书加密").font(theme::semibold(18.0)));
    ui.label("AES-256 · RSA 收件人证书 · 另存加密副本");
    ui.add_space(10.0);
    let d = &mut app.certificate_draft;
    ui.label("收件人证书 (.cer / .crt / .pem / .der)");
    egui::Frame::new().stroke(egui::Stroke::new(1.0, t.border)).corner_radius(6).inner_margin(8).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(300.0).hint_text("选择证书文件或填写路径"));
            #[cfg(not(target_arch = "wasm32"))]
            if widgets::pill_button(ui, "浏览", false).clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("收件人证书", &["cer", "crt", "pem", "der"]).pick_file()
            {
                d.path = path.to_string_lossy().into_owned();
            }
        });
    });
    if widgets::pill_button(ui, "添加收件人", false).clicked() {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let result = (|| {
                let bytes = read_bounded(&d.path, 64 * 1024)?;
                let certs = printcraft_engine::sign::x509::load_certificates(&bytes).map_err(|e| e.to_string())?;
                if certs.len() != 1 {
                    return Err("每次请选择一个收件人证书，不是证书链".into());
                }
                let cert = certs.into_iter().next().ok_or("文件中没有证书")?;
                let now =
                    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs().min(i64::MAX as u64)
                        as i64;
                printcraft_engine::sign::public_key::validate_recipient(&cert, now).map_err(|e| e.to_string())?;
                if d.recipients.len() >= 16 {
                    return Err("最多添加 16 个收件人".into());
                }
                if d.recipients.iter().any(|c| c.raw == cert.raw) {
                    return Err("这个证书已添加".into());
                }
                d.recipients.push(cert);
                d.path.clear();
                Ok(())
            })();
            d.error = result.err();
        }
    }
    let mut remove = None;
    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
        for (i, c) in d.recipients.iter().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(c.display_name()).strong());
                    if ui.small_button("移除").clicked() {
                        remove = Some(i);
                    }
                });
                ui.label(egui::RichText::new(format!("SHA-256: {}", c.fingerprint())).small());
            });
        }
    });
    if let Some(i) = remove {
        d.recipients.remove(i);
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.label("收件人权限");
        egui::ComboBox::from_id_salt("certificate-permissions")
            .width(240.0)
            .selected_text(if d.full_control { "完全控制（编辑、保存）" } else { "只读与打印" })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut d.full_control, false, "只读与打印");
                ui.selectable_value(&mut d.full_control, true, "完全控制（编辑、保存）");
            });
    });
    ui.label("打开副本需要对应的 .p12/.pfx 私钥及密码。权限限制取决于阅读器；这里不会联网验证证书是否已吊销。请核对指纹。");
    ui.checkbox(&mut d.acknowledged, "已确认收件人持有对应私钥，保留原 PDF");
    if let Some(e) = &d.error {
        ui.colored_label(egui::Color32::from_rgb(0xD1, 0x3B, 0x3B), e);
    }
    ui.add_space(10.0);
    let mut close = false;
    let mut save = false;
    ui.horizontal(|ui| {
        save = ui.add_enabled_ui(!d.recipients.is_empty() && d.acknowledged, |ui| widgets::pill_button(ui, "另存加密 PDF", true)).inner.clicked();
        close = widgets::pill_button(ui, "取消", false).clicked();
    });
    if save {
        close = app.save_certificate_copy();
    }
    close
}

impl PrintCraftApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn save_certificate_copy(&mut self) -> bool {
        let Some((_, id)) = self.active_ids() else { return false };
        let Some(doc) = self.session.get(id) else { return false };
        let path = match &self.certificate_draft.output_override {
            Some(p) => Some(std::path::PathBuf::from(p)),
            None => rfd::FileDialog::new()
                .set_file_name(format!("{}-证书加密.pdf", doc.name.trim_end_matches(".pdf")))
                .add_filter("PDF", &["pdf"])
                .save_file(),
        };
        let Some(path) = path else { return false };
        let result = (|| {
            if let Some(source) = &doc.path {
                let source = std::path::Path::new(source);
                if source == path || (source.canonicalize().ok().is_some() && source.canonicalize().ok() == path.canonicalize().ok()) {
                    return Err("请选择新文件名，不能覆盖原 PDF".into());
                }
            }
            let bytes = self.session.certificate_encrypted_bytes(id, &self.certificate_draft.recipients, self.certificate_draft.full_control)?;
            crate::editing::write_atomically(&path.to_string_lossy(), &bytes).map_err(|e| e.to_string())
        })();
        match result {
            Ok(()) => {
                self.notify(format!("已保存证书加密副本：{}；原文件未改动", path.display()));
                true
            }
            Err(e) => {
                self.certificate_draft.error = Some(e);
                false
            }
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn save_certificate_copy(&mut self) -> bool {
        self.certificate_draft.error = Some("证书加密需要桌面版".into());
        false
    }
}
