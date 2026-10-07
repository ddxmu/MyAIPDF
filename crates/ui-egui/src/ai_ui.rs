//! MyAIPDF's explicit, asynchronous AI workspace; document edits always require confirmation.
use crate::{PrintCraftApp, theme::Tokens};
use egui::RichText;
use printcraft_ai::{Action, Message, Provider, Reply};
use printcraft_engine::DocId;
use printcraft_engine::export::{ExportSource, Exporter};
use std::sync::{Arc, mpsc};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub providers: Vec<Provider>,
    pub selected: usize,
}
impl Default for Preferences {
    fn default() -> Self {
        Self { providers: vec![Provider::default()], selected: 0 }
    }
}
pub struct Pending {
    pub doc: DocId,
    pub bytes: Arc<Vec<u8>>,
    pub actions: Vec<Action>,
}
enum Event {
    Models { id: String, base: String, result: Result<Vec<String>, String> },
    Chat { doc: Option<(DocId, Arc<Vec<u8>>)>, result: Result<Reply, String> },
}
#[derive(Default)]
pub struct State {
    pub preferences: Preferences,
    pub input: String,
    pub history: Vec<Message>,
    pub include_text: bool,
    pub whole_document: bool,
    pub show_key: bool,
    pub status: String,
    pub pending: Option<Pending>,
    receiver: Option<mpsc::Receiver<Event>>,
    conversation_doc: Option<DocId>,
}
impl State {
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }
    pub fn restore(&mut self, value: &serde_json::Value) {
        if let Ok(mut p) = serde_json::from_value::<Preferences>(value.clone()) {
            p.providers.truncate(16);
            if p.providers.is_empty() {
                p.providers.push(Provider::default());
            }
            p.selected = p.selected.min(p.providers.len().saturating_sub(1));
            for provider in &mut p.providers {
                provider.models.truncate(4096);
                if provider.remember_key {
                    provider.api_key = printcraft_ai::load_key(provider).unwrap_or_default();
                }
            }
            self.preferences = p;
        }
    }
}

pub fn panel(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().id_salt("ai_workspace").auto_shrink([false, false]).show(ui, |ui| panel_content(app, ui));
}

fn panel_content(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.heading("AI 助手");
        if ui.small_button("返回工具").clicked() {
            app.left = crate::LeftPanel::AllTools;
        }
    });
    ui.label(RichText::new("阅读、总结、翻译，并协助操作 PDF").small().color(t.text_muted));
    let busy = app.ai.busy();
    egui::CollapsingHeader::new("AI 模型接口设置").default_open(true).show(ui, |ui| {
        ui.add_enabled_ui(!busy, |ui| {
            let selected = app.ai.preferences.selected;
            let name = app.ai.preferences.providers.get(selected).map(|p| p.name.clone()).unwrap_or_default();
            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("ai_provider").selected_text(name).width(210.0).show_ui(ui, |ui| {
                    for (i, p) in app.ai.preferences.providers.iter().enumerate() {
                        ui.selectable_value(&mut app.ai.preferences.selected, i, &p.name);
                    }
                });
                if app.ai.preferences.providers.len() < 16 && ui.small_button("新增接口").clicked() {
                    let n = app.ai.preferences.providers.len() + 1;
                    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or_default();
                    app.ai.preferences.providers.push(Provider {
                        id: format!("provider-{stamp}-{n}"),
                        name: format!("AI 接口 {n}"),
                        ..Provider::default()
                    });
                    app.ai.preferences.selected = n - 1;
                }
            });
            if selected != app.ai.preferences.selected {
                app.ai.history.clear();
                app.ai.pending = None;
                app.ai.status.clear();
            }
            let Some(p) = app.ai.preferences.providers.get_mut(app.ai.preferences.selected) else { return };
            let name_label = ui.label("接口名称");
            ui.add(egui::TextEdit::singleline(&mut p.name).id_salt("ai_name").desired_width(f32::INFINITY)).labelled_by(name_label.id);
            let url_label = ui.label("API 地址（OpenAI 兼容）");
            let old = p.base_url.clone();
            ui.add(egui::TextEdit::singleline(&mut p.base_url).id_salt("ai_url").hint_text("https://服务器/v1").desired_width(f32::INFINITY))
                .labelled_by(url_label.id);
            if old != p.base_url {
                p.api_key.clear();
                p.models.clear();
                p.model.clear();
                p.remember_key = false;
            }
            let key_label = ui.label("AI 密钥");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut p.api_key)
                        .password(!app.ai.show_key)
                        .id_salt("ai_key")
                        .hint_text("本地模型可留空")
                        .desired_width(265.0),
                )
                .labelled_by(key_label.id);
                ui.checkbox(&mut app.ai.show_key, "显示");
            });
            ui.checkbox(&mut p.remember_key, "保存密钥到 macOS 钥匙串");
            let fetch = ui.button("拉取模型").clicked();
            ui.horizontal(|ui| {
                ui.label("模型");
                egui::ComboBox::from_id_salt("ai_model_list")
                    .selected_text(if p.model.is_empty() { "请选择模型" } else { &p.model })
                    .width(265.0)
                    .height(220.0)
                    .show_ui(ui, |ui| {
                        for m in &p.models {
                            ui.selectable_value(&mut p.model, m.clone(), m);
                        }
                    });
            });
            let model_label = ui.label("模型 ID（可手动填写）");
            ui.add(egui::TextEdit::singleline(&mut p.model).id_salt("ai_model").hint_text("也可手动填写模型 ID").desired_width(f32::INFINITY))
                .labelled_by(model_label.id);
            ui.horizontal(|ui| {
                if ui.button("保存接口设置").clicked() {
                    let result = if p.remember_key { printcraft_ai::save_key(p) } else { printcraft_ai::forget_key(p) };
                    app.ai.status = match result {
                        Ok(()) => "接口已保存；密钥不写入普通配置文件".into(),
                        Err(e) => e,
                    };
                }
                if ui.small_button("忘记已存密钥").clicked() {
                    app.ai.status = match printcraft_ai::forget_key(p) {
                        Ok(()) => {
                            p.api_key.clear();
                            p.remember_key = false;
                            "已删除保存的密钥".into()
                        }
                        Err(e) => e,
                    };
                }
            });
            let provider = p.clone();
            if fetch {
                app.fetch_ai_models(provider);
            }
        });
    });
    ui.separator();
    ui.add_enabled_ui(!busy, |ui| {
        ui.checkbox(&mut app.ai.include_text, "发送 PDF 文字供 AI 分析");
        if app.ai.include_text {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut app.ai.whole_document, false, "当前页");
                ui.selectable_value(&mut app.ai.whole_document, true, "全文（最多 6 万字 / 150 页）");
            });
            ui.label(RichText::new("点击发送后，所选文字会提交至你配置的接口。").small().color(t.accent_text));
        } else {
            ui.label(RichText::new("仅发送问题和页数，不发送 PDF 文字。").small().color(t.text_muted));
        }
        ui.horizontal_wrapped(|ui| {
            for (label, prompt) in [
                ("总结要点", "请总结所选 PDF 范围的核心内容和要点。"),
                ("翻译中文", "请将所选 PDF 范围翻译为简体中文，保留关键术语。"),
                ("提取信息", "请提取关键数据、日期和待办事项。"),
                ("旋转页面", "请将当前页顺时针旋转 90 度，并给出待确认的操作。"),
            ] {
                if ui.small_button(label).clicked() {
                    app.ai.input = prompt.into();
                    if label != "旋转页面" {
                        app.ai.include_text = true;
                    }
                }
            }
        });
    });
    if !app.ai.status.is_empty() {
        ui.label(RichText::new(&app.ai.status).small().color(t.accent_text));
    }
    if busy {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("AI 正在处理…（接口超时 120 秒）");
        });
    }
    if let Some(pending) = &app.ai.pending {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new("待确认的 PDF 修改").strong());
            egui::ScrollArea::vertical().id_salt("ai_actions").max_height(140.0).show(ui, |ui| {
                for (i, a) in pending.actions.iter().enumerate() {
                    ui.label(format!("{}. {}", i + 1, a.label()));
                    ui.label(RichText::new(a.args.to_string()).small());
                }
            });
        });
        ui.label(RichText::new("只改当前文档，不会自动保存；可用 ⌘Z 撤销。").small().color(t.text_muted));
        let (mut apply, mut discard) = (false, false);
        ui.horizontal(|ui| {
            apply = ui.button("确认执行（不保存）").clicked();
            discard = ui.button("取消方案").clicked();
        });
        if apply {
            app.confirm_ai_plan();
        }
        if discard {
            app.ai.pending = None;
            app.ai.status = "已取消，文档未修改".into();
        }
    }
    let h = (ui.available_height() - 140.0).clamp(100.0, 280.0);
    egui::ScrollArea::vertical().id_salt("ai_chat_history").max_height(h).auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        if app.ai.history.is_empty() {
            ui.label(
                RichText::new("设置接口与模型后即可提问。\n例如：总结文档、翻译当前页、旋转第 2 页、填写表单。\nAI 回答仅供参考，请核对重要信息。")
                    .color(t.text_muted),
            );
        }
        for m in &app.ai.history {
            egui::Frame::NONE.fill(if m.role == "user" { t.accent_soft } else { t.card }).corner_radius(6).inner_margin(8).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(if m.role == "user" { "你" } else { "AI 助手" }).strong());
                ui.add(egui::Label::new(&m.content).wrap().selectable(true));
            });
            ui.add_space(7.0);
        }
    });
    let prompt_label = ui.label("问题或 PDF 处理要求");
    ui.add(
        egui::TextEdit::multiline(&mut app.ai.input)
            .id_salt("ai_prompt")
            .desired_rows(3)
            .desired_width(f32::INFINITY)
            .hint_text("输入问题或 PDF 处理要求…"),
    )
    .labelled_by(prompt_label.id);
    ui.horizontal(|ui| {
        if ui.add_enabled(!busy && !app.ai.input.trim().is_empty(), egui::Button::new("发送给 AI")).clicked() {
            app.send_ai();
        }
        if ui.add_enabled(!busy, egui::Button::new("清空对话")).clicked() {
            app.ai.history.clear();
            app.ai.pending = None;
            app.ai.status.clear();
        }
        ui.label(RichText::new("对话不落盘").small().color(t.text_muted));
    });
}

impl PrintCraftApp {
    pub fn fetch_ai_models(&mut self, provider: Provider) {
        if self.ai.busy() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.ai.receiver = Some(rx);
        self.ai.status = "正在拉取模型…".into();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let result = printcraft_engine::guard(|| printcraft_ai::fetch_models(&provider)).and_then(|r| r);
            let _ = tx.send(Event::Models { id: provider.id, base: provider.base_url, result });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }
    pub fn send_ai(&mut self) {
        if self.ai.busy() || self.ai.input.trim().is_empty() {
            return;
        }
        let Some(mut p) = self.ai.preferences.providers.get(self.ai.preferences.selected).cloned() else { return };
        if p.api_key.is_empty() && p.remember_key {
            p.api_key = printcraft_ai::load_key(&p).unwrap_or_default();
        }
        if let Err(e) = printcraft_ai::endpoint(&p.base_url, "chat/completions") {
            self.ai.status = e;
            return;
        }
        if p.model.is_empty() {
            self.ai.status = "请先选择模型或填写模型 ID".into();
            return;
        }
        let doc =
            self.active_ids().and_then(|(i, id)| self.session.get(id).map(|d| (id, d.export_source(), self.views.get(i).map_or(0, |v| v.current))));
        let id = doc.as_ref().map(|d| d.0);
        if self.ai.conversation_doc != id {
            self.ai.history.clear();
            self.ai.conversation_doc = id;
        }
        if self.ai.history.len() >= 40 {
            self.ai.status = "对话较长，请先清空对话".into();
            return;
        }
        if self.ai.input.chars().count() > 12_000 {
            self.ai.status = "单条问题最多 1.2 万字".into();
            return;
        }
        let prompt = std::mem::take(&mut self.ai.input);
        self.ai.history.push(Message { role: "user".into(), content: prompt });
        self.ai.pending = None;
        self.ai.status.clear();
        let history = self.ai.history.clone();
        let include = self.ai.include_text;
        let whole = self.ai.whole_document;
        let (tx, rx) = mpsc::channel();
        self.ai.receiver = Some(rx);
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let snapshot = doc.as_ref().map(|(id, src, _)| (*id, src.bytes.clone()));
            let tools = if doc.is_some() { printcraft_automation::Automation::ai_tools() } else { serde_json::json!([]) };
            let result = printcraft_engine::guard(|| {
                let text = match doc {
                    Some((_, src, current)) => context(src, current, include, whole)?,
                    None => String::new(),
                };
                printcraft_ai::chat(&p, &history, &text, &tools)
            })
            .and_then(|r| r);
            let _ = tx.send(Event::Chat { doc: snapshot, result });
            if let Some(ctx) = ctx {
                ctx.request_repaint();
            }
        });
    }
    pub(crate) fn poll_ai(&mut self) {
        let event = match self.ai.receiver.as_ref().map(mpsc::Receiver::try_recv) {
            Some(Ok(e)) => Some(e),
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.ai.status = "AI 请求意外终止，请重试".into();
                self.ai.receiver = None;
                None
            }
            _ => None,
        };
        let Some(event) = event else { return };
        self.ai.receiver = None;
        match event {
            Event::Models { id, base, result } => match result {
                Ok(models) => {
                    if let Some(p) = self.ai.preferences.providers.iter_mut().find(|p| p.id == id && p.base_url == base) {
                        let count = models.len();
                        if !models.contains(&p.model) {
                            p.model = models.first().cloned().unwrap_or_default();
                        }
                        p.models = models;
                        self.ai.status = format!("已获取 {count} 个模型，请选择对话模型");
                    }
                }
                Err(e) => self.ai.status = e,
            },
            Event::Chat { doc, result } => match result {
                Ok(reply) => {
                    self.ai.history.push(Message { role: "assistant".into(), content: reply.reply });
                    if !reply.actions.is_empty() {
                        if let Some((doc, bytes)) = doc {
                            self.ai.pending = Some(Pending { doc, bytes, actions: reply.actions });
                        } else {
                            self.ai.status = "未打开文档，AI 操作方案已忽略".into();
                        }
                    }
                }
                Err(e) => self.ai.status = e,
            },
        }
    }
    pub fn confirm_ai_plan(&mut self) {
        let Some(pending) = self.ai.pending.take() else { return };
        let same = self.active_ids().is_some_and(|(_, id)| id == pending.doc)
            && self.session.get(pending.doc).is_some_and(|d| Arc::ptr_eq(&d.export_source().bytes, &pending.bytes));
        if !same {
            self.ai.status = "文档或版本已变化，方案未执行；请重新提问".into();
            return;
        }
        let mut a = printcraft_automation::Automation::from_session(std::mem::take(&mut self.session));
        let result = printcraft_engine::guard(|| a.apply_ai_plan(pending.doc, &pending.actions)).and_then(|r| r.map_err(|e| e.to_string()));
        self.session = a.into_session();
        if let Some(d) = self.session.get(pending.doc) {
            for v in self.views.iter_mut().filter(|v| v.id == pending.doc) {
                v.document_changed(&d.info);
            }
        }
        self.ai.status = match result {
            Ok(()) => format!("已执行 {} 项修改，尚未保存；⌘Z 可撤销，建议另存为。", pending.actions.len()),
            Err(e) => e,
        };
        self.ai.history.push(Message { role: "assistant".into(), content: self.ai.status.clone() });
    }
}

fn context(src: ExportSource, current: usize, include: bool, whole: bool) -> Result<String, String> {
    let mut out = format!("文档总页数：{}；当前页：{}。\n", src.pages, current + 1);
    if !include {
        out.push_str("用户未授权发送文档文字。只能按指令提议操作，不能总结或翻译未提供的内容。");
        return Ok(out);
    }
    let count = src.pages;
    let pages: Vec<usize> = if whole { (0..count.min(150)).collect() } else { vec![current.min(count.saturating_sub(1))] };
    let mut exporter = Exporter::from_source(src);
    let mut text_chars = 0;
    for page in pages {
        let text = exporter.text(page)?;
        text_chars += text.chars().count();
        let left = printcraft_ai::MAX_CONTEXT_CHARS.saturating_sub(out.chars().count() + 80);
        out.push_str(&format!("\n--- 第 {} 页 ---\n{}\n", page + 1, text.chars().take(left).collect::<String>()));
        if text.chars().count() > left || out.chars().count() >= printcraft_ai::MAX_CONTEXT_CHARS.saturating_sub(100) {
            out.push_str("\n[已达到文字限制，后续内容未发送。]");
            break;
        }
    }
    if text_chars == 0 {
        return Err("未提取到文字：扫描件请先使用 OCR，或关闭发送文字仅提问".into());
    }
    if whole && count > 150 {
        out.push_str("\n[本次最多提取前 150 页，其余未发送。]");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_never_serializes_keys_or_conversations() {
        let mut app = PrintCraftApp::new();
        app.ai.preferences.providers[0].api_key = "private-key-test".into();
        app.ai.history.push(Message { role: "user".into(), content: "private-conversation".into() });
        let saved = app.persist();
        assert!(!saved.contains("private-key-test"));
        assert!(!saved.contains("private-conversation"));
        let mut restored = PrintCraftApp::new();
        restored.restore(&saved);
        assert_eq!(restored.ai.preferences.providers.len(), 1);
        assert!(restored.ai.preferences.providers[0].api_key.is_empty());
    }
}
