//! Local, nonmutating PDF to page-positioned Excel sheets.
pub use printcraft_export::spreadsheet::Mode as ExcelMode;

impl crate::Document {
    pub fn export_excel(&self, mode: ExcelMode) -> Result<Vec<u8>, String> {
        use printcraft_export::spreadsheet::{Fill, Page, Picture, Rule};
        if self.info.pages.is_empty() || self.info.pages.len() > 500 {
            return Err("Excel 导出支持 1 至 500 页，请分批导出".into());
        }
        let mut cos = self.editor.as_ref().map(|e| e.cos.clone());
        let mut renderer = crate::export::Exporter::new(self);
        let mut pages = Vec::new();
        let mut total = 0usize;
        for (i, info) in self.info.pages.iter().enumerate() {
            let (width, height) = (info.width as f64, info.height as f64);
            if !(7.2..=1584.0).contains(&width) || !(7.2..=1584.0).contains(&height) {
                return Err(format!("第 {} 页超出 Excel 支持的页面大小，未缩放原稿", i + 1));
            }
            let layout = if mode == ExcelMode::Editable { cos.as_mut().and_then(|d| printcraft_edit::sheet_export::layout(d, i).ok()) } else { None };
            let mut p = Page { width, height, text: Vec::new(), rules: Vec::new(), fills: Vec::new(), images: Vec::new(), png: None };
            let mut image_only = true;
            if let Some(layout) = layout
                && !layout.complex
            {
                let image_rects = layout.images;
                let raster_text = layout.raster_text;
                p.text = layout
                    .text
                    .into_iter()
                    .map(|t| printcraft_export::word::Text {
                        text: t.line.text,
                        rect: t.rect,
                        size: t.size,
                        font: t.line.base_font,
                        bold: t.line.bold,
                        italic: t.line.italic,
                        color: t.line.color,
                    })
                    .collect();
                p.rules = layout.rules.into_iter().map(|r| Rule { points: r.points, color: r.color, width: r.width }).collect();
                p.fills = layout.fills.into_iter().map(|f| Fill { rect: f.rect, color: f.color }).collect();
                image_only = false;
                if image_rects.len() != self.page_images(i).len() {
                    image_only = true;
                }
                for (k, rect) in image_rects.into_iter().enumerate() {
                    match self.page_image_file(i, k) {
                        Ok((ext, bytes)) => p.images.push(Picture { rect, bytes, ext, alt: "PDF 原始图片" }),
                        Err(_) => {
                            image_only = true;
                            break;
                        }
                    }
                }
                if !image_only && !raster_text.is_empty() {
                    for fragment in renderer.fragments(i, 200.0, &raster_text)? {
                        p.images.push(Picture {
                            rect: fragment.rect, bytes: fragment.png, ext: "png", alt: "PDF 特殊或裁切文字图像，不能逐字编辑"
                        });
                    }
                }
                if p.text.is_empty() {
                    image_only = true;
                }
                if printcraft_export::spreadsheet::validate(&p).is_err() {
                    image_only = true;
                }
            }
            if image_only {
                let png = renderer.png(i, 200.0)?;
                p.text.clear();
                p.rules.clear();
                p.fills.clear();
                p.images.clear();
                p.png = Some(png);
            }
            total = total.saturating_add(p.png.as_ref().map_or(0, Vec::len)).saturating_add(p.images.iter().map(|im| im.bytes.len()).sum::<usize>());
            if total > 256 * 1024 * 1024 {
                return Err("Excel 图像过大，请分批导出".into());
            }
            pages.push(p);
        }
        printcraft_export::spreadsheet::xlsx(&pages)
    }
}
