//! Watermark analysis and selection stay next to the document. No candidate is selected
//! automatically; deletion needs a second confirmation and remains one undoable edit.

use std::collections::BTreeSet;

use printcraft_engine::{DocId, Edit, WatermarkCandidate};

use crate::{Dialog, LeftPanel, PrintCraftApp, theme::Tokens, widgets};

#[derive(Default)]
pub struct WatermarkState {
    pub all_pages: bool,
    pub include_all: bool,
    pub filter: String,
    pub candidates: Vec<WatermarkCandidate>,
    pub selected: BTreeSet<String>,
    pub status: String,
    pub(crate) analyzed: Option<(DocId, u64, Vec<usize>)>,
}

impl PrintCraftApp {
    pub(crate) fn analyze_watermarks(&mut self) {
        let Some((i, id)) = self.active_ids() else {
            self.notify("请先打开 PDF");
            return;
        };
        let Some(doc) = self.session.get(id) else {
            return;
        };
        let pages = if self.watermarks.all_pages { (0..doc.info.pages.len()).collect() } else { vec![self.views[i].current] };
        match doc.watermark_candidates(&pages, self.watermarks.include_all) {
            Ok(found) => {
                self.watermarks.analyzed = Some((id, doc.edit_generation(), pages));
                self.watermarks.status = if found.is_empty() {
                    "未发现可独立删除的候选。可勾选“显示所有独立对象”再分析；扫描图片中的水印无法直接分离。".into()
                } else {
                    format!("发现 {} 组候选，请预览并勾选要删除的水印。", found.len())
                };
                self.watermarks.candidates = found;
                self.watermarks.selected.clear();
            }
            Err(e) => {
                self.watermarks.analyzed = None;
                self.watermarks.candidates.clear();
                self.watermarks.selected.clear();
                self.watermarks.status = e;
            }
        }
    }

    pub(crate) fn remove_selected_watermarks(&mut self) {
        let Some((id, generation, pages)) = self.watermarks.analyzed.clone() else {
            return;
        };
        if self.active_ids().map(|(_, d)| d) != Some(id) || self.session.get(id).is_none_or(|d| d.edit_generation() != generation) {
            self.watermarks.status = "文档已改变，请重新分析水印。未删除任何内容。".into();
            return;
        }
        let ids = self.watermarks.selected.iter().cloned().collect();
        if self.apply_edit(Edit::RemoveWatermarks { pages, candidates: ids }) {
            self.analyze_watermarks();
            self.watermarks.status = "已删除所选水印。可按 ⌘Z 撤销；请保存或另存为以保留结果。".into();
            self.notify("所选水印已删除，原文件尚未覆盖；可撤销或另存为。");
        }
    }
}

pub(crate) fn panel(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    // Unchecked boxes/radios and the filter must remain visible on a white panel.
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.bg_stroke = egui::Stroke::new(1.0, t.border);
    w.inactive.bg_fill = t.field;
    w.inactive.weak_bg_fill = t.field;
    w.hovered.bg_stroke = egui::Stroke::new(1.2, t.accent);
    w.active.bg_stroke = egui::Stroke::new(1.2, t.accent);
    ui.horizontal(|ui| {
        if widgets::ghost_button(ui, "chevron-left", "返回工具").on_hover_text("返回工具菜单").clicked() {
            app.left = LeftPanel::AllTools;
        }
        ui.heading("水印去除");
    });
    ui.label(egui::RichText::new("1 分析  ·  2 选择  ·  3 删除").color(t.accent_text));
    ui.add_space(8.0);
    ui.label("仅处理你有权修改的文档。候选也可能是正文、页眉或插图，删除前请预览确认。");
    let Some((index, id)) = app.active_ids() else {
        ui.label("打开 PDF 后可分析水印。");
        return;
    };
    let generation = app.session.get(id).map_or(0, |d| d.edit_generation());
    let stale = app.watermarks.analyzed.as_ref().is_some_and(|(d, g, _)| *d != id || *g != generation);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.radio_value(&mut app.watermarks.all_pages, false, "当前页").changed()
            | ui.radio_value(&mut app.watermarks.all_pages, true, "全部页面").changed()
        {
            app.watermarks.analyzed = None;
            app.watermarks.selected.clear();
            app.watermarks.candidates.clear();
        }
    });
    if ui.checkbox(&mut app.watermarks.include_all, "显示所有独立对象（人工识别）").changed() {
        app.watermarks.analyzed = None;
        app.watermarks.selected.clear();
        app.watermarks.candidates.clear();
    }
    if widgets::pill_button(ui, "分析水印", true).clicked() {
        app.analyze_watermarks();
    }
    ui.add_space(8.0);
    let label = ui.label("按水印文字筛选");
    ui.add(
        egui::TextEdit::singleline(&mut app.watermarks.filter)
            .margin(egui::vec2(8.0, 6.0))
            .desired_width(f32::INFINITY)
            .hint_text("输入水印文字或对象类型"),
    )
    .labelled_by(label.id);
    if stale {
        ui.colored_label(t.accent_text, "文档已改变，请重新分析。旧选择不能删除。");
    }
    if !app.watermarks.status.is_empty() {
        ui.label(&app.watermarks.status);
    }
    ui.add_space(6.0);
    let mut preview = None;
    let filter = app.watermarks.filter.to_lowercase();
    egui::ScrollArea::vertical().id_salt("watermark-candidates").max_height((ui.available_height() - 115.0).max(60.0)).show(ui, |ui| {
        for c in app.watermarks.candidates.iter().filter(|c| filter.is_empty() || c.label.to_lowercase().contains(&filter)) {
            ui.push_id(&c.id, |ui| {
                egui::Frame::new().fill(t.field).stroke(egui::Stroke::new(1.0, t.border)).corner_radius(8).inner_margin(8).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let mut checked = app.watermarks.selected.contains(&c.id);
                    if ui.add_enabled(!stale, egui::Checkbox::new(&mut checked, &c.label)).changed() {
                        if checked {
                            app.watermarks.selected.insert(c.id.clone());
                        } else {
                            app.watermarks.selected.remove(&c.id);
                        }
                    }
                    ui.label(egui::RichText::new(&c.reason).small().color(t.text_muted));
                    let pages: BTreeSet<_> = c.occurrences.iter().map(|o| o.page + 1).collect();
                    ui.label(format!(
                        "{} 处 · 第 {} 页",
                        c.occurrences.len(),
                        pages.iter().take(12).map(usize::to_string).collect::<Vec<_>>().join("、")
                    ));
                    if ui.button("预览位置").clicked() {
                        preview = c.occurrences.first().cloned();
                    }
                });
            });
            ui.add_space(6.0);
        }
    });
    if let Some(o) = preview
        && let Some(info) = app.session.get(id).and_then(|d| d.info.pages.get(o.page))
    {
        let a = info.view_to_user(o.rect[0] as f32, o.rect[1] as f32);
        let b = info.view_to_user(o.rect[2] as f32, o.rect[3] as f32);
        app.views[index].go_to_page(o.page);
        app.views[index].flash = Some((o.page, [a[0].min(b[0]), a[1].min(b[1]), a[0].max(b[0]), a[1].max(b[1])], 0.0));
    }
    ui.separator();
    ui.label(format!("已选择 {} 组水印", app.watermarks.selected.len()));
    if ui
        .add_enabled(!stale && !app.watermarks.selected.is_empty(), egui::Button::new("删除所选水印").fill(t.accent).stroke(egui::Stroke::NONE))
        .clicked()
    {
        app.dialog = Some(Dialog::RemoveWatermarks);
    }
    ui.label(egui::RichText::new("可撤销 · 手动保存 · 不删除扫描页背景").small().color(t.text_muted));
}

pub(crate) fn confirm(ui: &mut egui::Ui, app: &PrintCraftApp) -> (bool, bool) {
    ui.heading("确认删除水印");
    ui.label(format!("删除已选择的 {} 组对象？未选择的正文和图片将保留。", app.watermarks.selected.len()));
    ui.label("这是候选分析，不保证每个候选都是水印。请先预览位置。操作可撤销，原文件需手动保存。");
    ui.add_space(12.0);
    let mut result = (false, false);
    ui.horizontal(|ui| {
        result.0 = ui.button("确认删除").clicked();
        result.1 = ui.button("取消").clicked();
    });
    result
}
