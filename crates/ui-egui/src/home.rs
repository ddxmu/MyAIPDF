//! Home tab: recommended tools, open card, recent files (local only, never another app's list).

use egui::{Align2, CornerRadius, Rect, Sense, Stroke, vec2};
use printcraft_engine::catalog;

use crate::theme::{self, Tokens};
use crate::{LeftPanel, PrintCraftApp, icons, panels::human_size, widgets};

const RECOMMENDED: [&str; 5] = ["organize", "comment", "form", "edit", "protect"];

pub fn show(app: &mut PrintCraftApp, ui: &mut egui::Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        egui::Frame::NONE.inner_margin(egui::Margin { left: 36, right: 36, top: 28, bottom: 28 }).show(ui, |ui| {
            ui.label(egui::RichText::new(crate::i18n::ui_tr(ui, "Welcome to MyAIPDF")).font(theme::semibold(24.0)));
            ui.label(
                egui::RichText::new(crate::i18n::ui_tr(ui, "开源 PDF 工作台 · 本地处理 · 中文界面 · AI 辅助"))
                    .color(t.text_muted)
                    .font(theme::regular(14.0)),
            );
            ui.add_space(14.0);
            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        widgets::myaipdf_mark(ui, 36.0);
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(crate::i18n::ui_tr(ui, "让 AI 协助您处理 PDF")).font(theme::semibold(15.0)));
                            ui.label(
                                egui::RichText::new(crate::i18n::ui_tr(ui, "配置 API 地址、密钥和模型，询问文档或生成待确认的编辑操作。"))
                                    .color(t.text_muted),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    if widgets::icon_pill(ui, "sparkles", "打开 AI 助手", true).clicked() {
                        app.execute("ai.ask");
                    }
                });
            ui.add_space(22.0);

            egui::Frame::NONE
                .fill(t.card)
                .stroke(Stroke::new(1.0, t.border))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(egui::Margin::same(18))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(egui::RichText::new(crate::i18n::ui_tr(ui, "Recommended tools")).font(theme::semibold(15.0)));
                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(14.0, 14.0);
                        for id in RECOMMENDED {
                            let Some(g) = catalog::group(id) else { continue };
                            let (rect, resp) = ui.allocate_exact_size(vec2(190.0, 104.0), Sense::click());
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, app.language.tr(g.label)));
                            let fill = if resp.hovered() { t.hover } else { t.card };
                            ui.painter().rect(rect, CornerRadius::same(10), fill, Stroke::new(1.0, t.divider), egui::StrokeKind::Inside);
                            let color = egui::Color32::from_rgb(g.hue[0], g.hue[1], g.hue[2]);
                            icons::paint(ui, Rect::from_min_size(rect.min + vec2(14.0, 14.0), vec2(22.0, 22.0)), g.icon, 21.0, color);
                            ui.painter().text(
                                rect.min + vec2(44.0, 25.0),
                                Align2::LEFT_CENTER,
                                app.language.tr(g.label),
                                theme::semibold(13.5),
                                t.text,
                            );
                            let blurb = if id == "edit" {
                                "文字与字号 · 图片 · 保存".to_string()
                            } else {
                                g.sections
                                    .first()
                                    .map(|s| s.items.iter().take(3).map(|i| app.language.tr(i.label)).collect::<Vec<_>>().join(" · "))
                                    .unwrap_or_default()
                            };
                            let galley = ui.fonts_mut(|f| f.layout(blurb, theme::regular(11.5), t.text_muted, rect.width() - 28.0));
                            ui.painter().galley(rect.min + vec2(14.0, 46.0), galley, t.text_muted);
                            ui.painter().text(
                                rect.left_bottom() + vec2(14.0, -14.0),
                                Align2::LEFT_CENTER,
                                app.language.tr("Use now"),
                                theme::medium(12.0),
                                t.accent_text,
                            );
                            if resp.clicked() {
                                app.left = LeftPanel::Tool(g.id);
                                app.left_open = true;
                            }
                        }
                        let (rect, resp) = ui.allocate_exact_size(vec2(170.0, 104.0), Sense::click());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, app.language.tr("Open file")));
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(10),
                            if resp.hovered() { t.hover } else { t.pasteboard },
                            Stroke::new(1.0, t.divider),
                            egui::StrokeKind::Inside,
                        );
                        icons::paint(ui, Rect::from_center_size(rect.center() - vec2(0.0, 16.0), vec2(28.0, 28.0)), "folder-open", 26.0, t.icon);
                        ui.painter().text(
                            rect.center() + vec2(0.0, 22.0),
                            Align2::CENTER_CENTER,
                            app.language.tr("Open file"),
                            theme::semibold(13.0),
                            t.text,
                        );
                        if resp.clicked() {
                            app.open_dialog();
                        }
                    });
                });

            ui.add_space(26.0);
            ui.label(egui::RichText::new(crate::i18n::ui_tr(ui, "Recent")).font(theme::semibold(17.0)));
            ui.add_space(8.0);
            if app.recent.is_empty() {
                ui.label(egui::RichText::new(crate::i18n::ui_tr(ui, "最近打开的文件将显示在这里。可将 PDF 拖入窗口打开。")).color(t.text_muted));
            }
            let mut open = None;
            for r in &app.recent {
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &r.name));
                if resp.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(8), t.hover);
                }
                icons::paint(
                    ui,
                    Rect::from_min_size(rect.min + vec2(10.0, 11.0), vec2(24.0, 24.0)),
                    "file-text",
                    22.0,
                    egui::Color32::from_rgb(0xE0, 0x3E, 0x3E),
                );
                let details = format!("{} 页  ·  {}", r.pages, human_size(r.size));
                let details_width = ui.fonts_mut(|f| f.layout_no_wrap(details.clone(), theme::regular(12.0), t.text_muted).size().x);
                let text_width = (rect.width() - 46.0 - details_width - 32.0).max(0.0);
                for (text, y, font, color) in [(&r.name, 15.0, theme::medium(13.5), t.text), (&r.path, 32.0, theme::regular(11.0), t.text_faint)] {
                    let mut job = egui::text::LayoutJob::simple(text.clone(), font, color, text_width);
                    job.wrap.max_rows = 1;
                    job.wrap.break_anywhere = true;
                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                    ui.painter().galley(rect.min + vec2(46.0, y - galley.size().y / 2.0), galley, color);
                }
                ui.painter().text(rect.right_center() - vec2(12.0, 0.0), Align2::RIGHT_CENTER, details, theme::regular(12.0), t.text_muted);
                let resp = resp.on_hover_text(&r.path);
                if resp.clicked() {
                    open = Some(r.path.clone());
                }
            }
            if let Some(p) = open {
                if let Some(i) = app.views.iter().position(|v| app.session.get(v.id).and_then(|d| d.path.as_deref()) == Some(p.as_str())) {
                    app.active = Some(i);
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    app.open_path(&p);
                }
            }
            ui.add_space(20.0);
            widgets::section_title(ui, "Privacy");
            ui.label(
                egui::RichText::new(crate::i18n::ui_tr(
                    ui,
                    "PDF 操作在本机完成，无需账号，无遥测。AI 仅在主动发送时连接您配置的接口；文档文字需勾选授权后才发送。",
                ))
                .color(t.text_muted),
            );
        });
    });
}
