//! Honest, working controls for the small production, scan and measuring utilities.
use crate::{PrintCraftApp, QuickTool, widgets};
use printcraft_engine::{Edit, ProductionSettings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Transitions,
    Gray,
    Hairlines,
    PrinterMarks,
    EnhanceScans,
    Measure,
}

impl Kind {
    pub fn title(self) -> &'static str {
        match self {
            Self::Transitions => "页面切换",
            Self::Gray => "文字与矢量转灰度",
            Self::Hairlines => "修复细线",
            Self::PrinterMarks => "添加印刷裁切标记",
            Self::EnhanceScans => "增强扫描件",
            Self::Measure => "测量距离",
        }
    }
}

pub struct UtilityDraft {
    pub all_pages: bool,
    pub transition: String,
    pub seconds: f64,
    pub minimum: f64,
    pub margin: f64,
    pub contrast: f32,
    pub sharpen: bool,
    pub from: [f64; 2],
    pub to: [f64; 2],
    pub ratio: f64,
    pub unit: String,
}
impl Default for UtilityDraft {
    fn default() -> Self {
        Self {
            all_pages: false,
            transition: "Dissolve".into(),
            seconds: 1.0,
            minimum: 0.5,
            margin: 36.0,
            contrast: 20.0,
            sharpen: true,
            from: [20.0, 20.0],
            to: [100.0, 100.0],
            ratio: 1.0,
            unit: "mm".into(),
        }
    }
}

impl PrintCraftApp {
    pub(crate) fn apply_utility(&mut self, kind: Kind) {
        let Some((i, id)) = self.active_ids() else {
            return;
        };
        let Some(doc) = self.session.get(id) else {
            return;
        };
        let pages = if self.utilities.all_pages { (0..doc.info.pages.len()).collect() } else { self.views[i].target_pages() };
        let d = &self.utilities;
        if kind == Kind::EnhanceScans {
            match self.session.enhance_scans(id, &pages, d.contrast, d.sharpen) {
                Ok(n) => {
                    if let Some(info) = self.session.get(id).map(|d| d.info.clone()) {
                        self.views[i].document_changed(&info);
                    }
                    self.notify(format!("已增强 {n} 张扫描背景；可撤销，请手动保存。"));
                }
                Err(e) => self.notify(e),
            }
            return;
        }
        if kind == Kind::Measure {
            match doc.measure_distance(self.views[i].current, d.from, d.to, d.ratio, &d.unit) {
                Ok(n) => self.notify(format!("距离 {n:.3} {}（比例 1:{}）。选择“页面拖拽测量”可留下测量标注。", d.unit, d.ratio)),
                Err(e) => self.notify(e),
            }
            return;
        }
        let settings = match kind {
            Kind::Transitions => ProductionSettings::Transitions { style: d.transition.clone(), seconds: d.seconds },
            Kind::Gray => ProductionSettings::VectorGray,
            Kind::Hairlines => ProductionSettings::Hairlines { minimum: d.minimum },
            Kind::PrinterMarks => ProductionSettings::PrinterMarks { margin: d.margin },
            _ => return,
        };
        if self.apply_edit(Edit::PageProduction { pages, settings }) {
            self.notify("已完成页面处理。可撤销；请手动保存或另存为。");
        }
    }
}

pub(crate) fn body(ui: &mut egui::Ui, app: &mut PrintCraftApp, kind: Kind) -> (bool, bool) {
    ui.heading(kind.title());
    ui.add_space(8.0);
    let mut draw = false;
    let active = app.active_ids();
    let d = &mut app.utilities;
    if kind != Kind::Measure {
        ui.horizontal(|ui| {
            ui.radio_value(&mut d.all_pages, false, "当前页 / 已选页面");
            ui.radio_value(&mut d.all_pages, true, "全部页面");
        });
    }
    match kind {
        Kind::Transitions => {
            ui.label("写入 PDF 的页面切换效果，供支持演示模式的阅读器使用。MyAIPDF 阅读模式不会强行播放动画。");
            egui::ComboBox::from_id_salt("transition-style")
                .selected_text(match d.transition.as_str() {
                    "none" => "无",
                    "Fade" => "淡入",
                    "Wipe" => "擦除",
                    _ => "溶解",
                })
                .show_ui(ui, |ui| {
                    for (v, l) in [("none", "无"), ("Dissolve", "溶解"), ("Fade", "淡入"), ("Wipe", "擦除")] {
                        ui.selectable_value(&mut d.transition, v.into(), l);
                    }
                });
            ui.horizontal(|ui| {
                ui.label("持续时间");
                ui.add(egui::DragValue::new(&mut d.seconds).range(0.1..=30.0).suffix(" 秒"));
            });
        }
        Kind::Gray => {
            ui.label("将页面文字和矢量的直接 RGB / CMYK 颜色转为灰度，包含嵌套图形。");
            ui.label("不转换扫描图片、ICC/专色或批注外观；不是专业印刷色彩管理或分色证明。");
        }
        Kind::Hairlines => {
            ui.label("提高内容流和嵌套图形中的显式细线宽度，避免零宽线。单位为 PDF 内容坐标点；图形缩放可能影响最终显示宽度。");
            ui.horizontal(|ui| {
                ui.label("最小线宽");
                ui.add(egui::DragValue::new(&mut d.minimum).range(0.01..=10.0).suffix(" pt"));
            });
        }
        Kind::PrinterMarks => {
            ui.label("扩展纸张边缘并添加四角裁切线。原裁切区域记为 TrimBox，正文保持原位置。");
            ui.horizontal(|ui| {
                ui.label("新增边距");
                ui.add(egui::DragValue::new(&mut d.margin).range(18.0..=144.0).suffix(" pt"));
            });
        }
        Kind::EnhanceScans => {
            ui.label("调整独立扫描背景的对比度和锐度。保留已有 OCR 文字层，不把正文页面变成图片；本次最多 100 页。");
            ui.horizontal(|ui| {
                ui.label("对比度");
                ui.add(egui::Slider::new(&mut d.contrast, -50.0..=100.0));
            });
            ui.checkbox(&mut d.sharpen, "轻度锐化");
        }
        Kind::Measure => {
            ui.label("页面左上角为坐标原点。可输入两点计算，也可直接在页面拖拽并保存测量标注。");
            for (label, p) in [("起点", &mut d.from), ("终点", &mut d.to)] {
                ui.horizontal(|ui| {
                    ui.label(label);
                    ui.label("X");
                    ui.add(egui::DragValue::new(&mut p[0]).range(0.0..=100000.0));
                    ui.label("Y");
                    ui.add(egui::DragValue::new(&mut p[1]).range(0.0..=100000.0));
                });
            }
            ui.horizontal(|ui| {
                ui.label("比例 1:");
                ui.add(egui::DragValue::new(&mut d.ratio).range(0.000001..=1000000.0));
                egui::ComboBox::from_id_salt("measure-unit").selected_text(&d.unit).show_ui(ui, |ui| {
                    for u in ["mm", "cm", "in", "pt"] {
                        ui.selectable_value(&mut d.unit, u.into(), u);
                    }
                });
            });
            if let Some((i, id)) = active
                && let Some(doc) = app.session.get(id)
                && let Ok(n) = doc.measure_distance(app.views[i].current, d.from, d.to, d.ratio, &d.unit)
            {
                ui.label(egui::RichText::new(format!("距离：{n:.3} {}", d.unit)).strong());
            }
            draw = ui.button("页面拖拽测量").clicked();
        }
    }
    ui.add_space(12.0);
    ui.separator();
    let mut result = (false, false);
    ui.horizontal(|ui| {
        result.0 = widgets::pill_button(ui, if kind == Kind::Measure { "计算距离" } else { "应用处理" }, true).clicked();
        result.1 = ui.button("取消").clicked();
    });
    if draw {
        app.quick_tool = QuickTool::Measure;
        app.notify("在 PDF 页面上拖拽两点进行测量。生成线段批注，可撤销并保存。");
        result.1 = true;
    }
    result
}
