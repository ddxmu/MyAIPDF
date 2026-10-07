//! Small document utilities exposed identically to the native UI and headless tools.

use crate::{DocId, Edit, ImageEdit, Session};
use std::sync::Arc;

impl Session {
    /// Convert supported files without opening tabs or altering current documents.
    pub fn file_as_pdf(&self, name: &str, bytes: Vec<u8>) -> Result<Arc<Vec<u8>>, String> {
        if bytes.starts_with(b"%PDF-") {
            return Ok(Arc::new(bytes));
        }
        if name.to_lowercase().ends_with(".txt") {
            let text = String::from_utf8(bytes).map_err(|_| "文本文件须为 UTF-8 编码".to_string())?;
            if text.chars().any(|c| c != '?' && printcraft_fonts::win_ansi(&c.to_string()) == b"?") {
                return Err(
                    "文本创建目前只支持可写入的拉丁字符；这份文件含中文或其他未支持字形，未替换成问号，也未创建丢字的 PDF。请先转为 PDF 再合并。"
                        .into(),
                );
            }
            return self.create_from_text(name, &text).map_err(|e| e.to_string());
        }
        self.create_from_images(&[(name.to_owned(), bytes)]).map_err(|e| e.to_string())
    }

    pub fn create_from_files(&self, files: Vec<(String, Vec<u8>)>) -> Result<Arc<Vec<u8>>, String> {
        if files.is_empty() || files.len() > 100 {
            return Err("请选择 1 至 100 个 PDF、图片或 UTF-8 文本文件".into());
        }
        if files.iter().fold(0usize, |n, (_, b)| n.saturating_add(b.len())) > 256 * 1024 * 1024 {
            return Err("文件总量过大，请分批合并".into());
        }
        let sources = files.into_iter().map(|(n, b)| self.file_as_pdf(&n, b).map(|v| (n, v, None))).collect::<Result<Vec<_>, _>>()?;
        self.combine_ranges(&sources).map_err(|e| e.to_string())
    }

    /// Scanned page enhancement changes only independent full-page images. Text/vector
    /// pages are skipped rather than rasterized; OCR text and page geometry are retained.
    pub fn enhance_scans(&mut self, id: DocId, pages: &[usize], contrast: f32, sharpen: bool) -> Result<usize, String> {
        if !contrast.is_finite() || !(-50.0..=100.0).contains(&contrast) || pages.is_empty() || pages.len() > 100 {
            return Err("请选择 1 至 100 页，对比度须在 -50 至 100 之间".into());
        }
        let doc = self.get(id).ok_or("文档已关闭")?;
        if !doc.allows_modification() {
            return Err("文档不允许修改".into());
        }
        let mut edits = Vec::new();
        let mut total = 0usize;
        let mut seen = std::collections::BTreeSet::new();
        for &page in pages {
            if !seen.insert(page) {
                return Err("页码重复".into());
            }
            let p = doc.info.pages.get(page).ok_or("页码超出范围")?;
            for (index, image) in doc.page_images(page).iter().enumerate() {
                let area = (image.rect[2] - image.rect[0]).abs() * (image.rect[3] - image.rect[1]).abs();
                if area < f64::from(p.width * p.height) * 0.8 {
                    continue;
                }
                if u64::from(image.width) * u64::from(image.height) > 20_000_000 {
                    return Err("扫描图片超过 2000 万像素，请先降低分辨率".into());
                }
                let (_, bytes) = doc.page_image_file(page, index)?;
                let reader = || image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().map_err(|e| e.to_string());
                let (width, height) = reader()?.into_dimensions().map_err(|e| e.to_string())?;
                if u64::from(width) * u64::from(height) > 20_000_000 {
                    return Err("扫描图片超过 2000 万像素，请先降低分辨率".into());
                }
                let mut input = reader()?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(width);
                limits.max_image_height = Some(height);
                limits.max_alloc = Some(160 * 1024 * 1024);
                input.limits(limits);
                let decoded = input.decode().map_err(|e| e.to_string())?;
                let mut rgb = decoded.to_rgb8();
                rgb = image::imageops::contrast(&rgb, contrast);
                if sharpen {
                    rgb = image::imageops::unsharpen(&rgb, 1.0, 2);
                }
                let rgba: Vec<_> = rgb.pixels().flat_map(|p| [p[0], p[1], p[2], 255]).collect();
                let png = crate::export::encode_png(rgb.width(), rgb.height(), &rgba)?;
                total = total.saturating_add(png.len());
                if total > 256 * 1024 * 1024 {
                    return Err("处理后图片总量过大，请分批增强".into());
                }
                edits.push(Edit::EditPageImage { page, index, change: ImageEdit::Replace { name: "enhanced.png".into(), bytes: Arc::new(png) } });
            }
        }
        let count = edits.len();
        if count == 0 {
            return Err("所选页面没有可增强的独立扫描背景，未改动正文或插图".into());
        }
        self.apply(id, Edit::Batch { label: "Enhance scanned pages".into(), edits }).map_err(|e| e.to_string())?;
        Ok(count)
    }
}

impl crate::Document {
    pub fn measure_distance(&self, page: usize, from: [f64; 2], to: [f64; 2], ratio: f64, unit: &str) -> Result<f64, String> {
        let p = self.info.pages.get(page).ok_or("页码超出范围")?;
        if !ratio.is_finite() || !(0.000001..=1_000_000.0).contains(&ratio) || from.iter().chain(to.iter()).any(|v| !v.is_finite()) {
            return Err("测量坐标或比例无效".into());
        }
        for [x, y] in [from, to] {
            if x < 0.0 || x > f64::from(p.width) || y < 0.0 || y > f64::from(p.height) {
                return Err("测量点须在页面范围内".into());
            }
        }
        let from = p.view_to_user(from[0] as f32, from[1] as f32).map(f64::from);
        let to = p.view_to_user(to[0] as f32, to[1] as f32).map(f64::from);
        self.measure_user_distance(page, from, to, ratio, unit)
    }

    pub fn measure_user_distance(&self, page: usize, from: [f64; 2], to: [f64; 2], ratio: f64, unit: &str) -> Result<f64, String> {
        let p = self.info.pages.get(page).ok_or("页码超出范围")?;
        for [x, y] in [from, to] {
            if !x.is_finite()
                || !y.is_finite()
                || x < f64::from(p.crop[0])
                || x > f64::from(p.crop[2])
                || y < f64::from(p.crop[1])
                || y > f64::from(p.crop[3])
            {
                return Err("测量点须在页面内".into());
            }
        }
        self.measure_length(page, from, to, ratio, unit)
    }

    fn measure_length(&self, page: usize, from: [f64; 2], to: [f64; 2], ratio: f64, unit: &str) -> Result<f64, String> {
        if !ratio.is_finite() || !(0.000001..=1_000_000.0).contains(&ratio) {
            return Err("比例无效".into());
        }
        let factor = match unit {
            "pt" => 1.0,
            "mm" => 25.4 / 72.0,
            "cm" => 2.54 / 72.0,
            "in" => 1.0 / 72.0,
            _ => return Err("支持 pt、mm、cm、in 单位".into()),
        };
        let scale = self
            .editor
            .as_ref()
            .and_then(|e| {
                printcraft_model::pages(&e.cos).into_iter().nth(page).and_then(|p| p.dict.get(b"UserUnit").and_then(|o| e.cos.resolve(o).as_f64()))
            })
            .unwrap_or(1.0);
        if !scale.is_finite() || scale <= 0.0 || scale > 75000.0 {
            return Err("PDF UserUnit 无效".into());
        }
        Ok((to[0] - from[0]).hypot(to[1] - from[1]) * factor * ratio * scale)
    }
}
