//! Export a PDF ▸ Image and Text (execution plan M10.3, the first formats).
//!
//! [`Exporter`] renders pages to PNG, JPEG or TIFF at a resolution and extracts reading-order
//! text, from a
//! document's working file (so unsaved edits and hidden layers are respected, as on screen).

use printcraft_render::{PageRenderer, RenderRequest, RequestKind};

use crate::Document;

/// Renders and extracts pages of one document state.
pub struct Exporter {
    renderer: PageRenderer,
    pages: usize,
    sizes: Vec<[f32; 2]>,
}

/// What an export needs from a document, as plain values that can move to a worker thread.
#[derive(Clone)]
pub struct ExportSource {
    pub bytes: std::sync::Arc<Vec<u8>>,
    pub config: printcraft_render::RenderConfig,
    pub pages: usize,
    pub sizes: Vec<[f32; 2]>,
}

impl Document {
    /// The current state, for exporting on another thread.
    pub fn export_source(&self) -> ExportSource {
        ExportSource {
            bytes: self.bytes.clone(),
            config: self.config.clone(),
            pages: self.info.pages.len(),
            sizes: self.info.pages.iter().map(|p| [p.width, p.height]).collect(),
        }
    }
}

/// Export all images: the images `pages` (0-based) use, each once, skipping those under
/// `min_side` pixels on their shorter side. JPEGs come out unchanged, other images as PNG.
pub fn extract_images(src: &ExportSource, pages: &[usize], min_side: u32) -> Result<printcraft_create::ImageExport, String> {
    let doc = printcraft_cos::Document::open_with_password(src.bytes.clone(), src.config.password.as_deref()).map_err(|e| e.to_string())?;
    Ok(printcraft_create::extract_images(&doc, pages, min_side))
}

/// The file name for the `index`-th (1-based) exported image: `<stem>_Page_<n>_Image_<index>.<ext>`.
pub fn image_file_name(stem: &str, image: &printcraft_create::ExtractedImage, index: usize) -> String {
    format!("{stem}_Page_{}_Image_{index:04}.{}", image.page + 1, image.extension)
}

impl Exporter {
    pub fn new(doc: &Document) -> Self {
        Self::from_source(doc.export_source())
    }

    pub fn from_source(src: ExportSource) -> Self {
        Self { renderer: PageRenderer::new(src.bytes, src.config), pages: src.pages, sizes: src.sizes }
    }

    fn check(&self, page: usize) -> Result<(), String> {
        if page < self.pages { Ok(()) } else { Err(format!("page {} does not exist", page + 1)) }
    }

    /// Raster PostScript Level 2; EPS contains exactly one page. Text and vectors become pixels.
    /// Page geometry includes crop, rotation and UserUnit, just like the displayed PDF.
    pub fn postscript(&mut self, pages: &[usize], dpi: f64, eps: bool) -> Result<Vec<u8>, String> {
        use std::fmt::Write as _;
        if pages.is_empty() || pages.len() > 500 || (eps && pages.len() != 1) {
            return Err("select 1–500 pages; EPS requires exactly one page".into());
        }
        if !dpi.is_finite() || !(18.0..=1200.0).contains(&dpi) {
            return Err("resolution must be between 18 and 1200 dpi".into());
        }
        let mut out =
            format!("%!PS-Adobe-3.0{}\n%%Creator: MyAIPDF\n%%LanguageLevel: 2\n%%Pages: {}\n", if eps { " EPSF-3.0" } else { "" }, pages.len());
        for (k, &page) in pages.iter().enumerate() {
            self.check(page)?;
            let &[w, h] = self.sizes.get(page).ok_or("page size is unavailable")?;
            if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 || w.max(h) > 1_000_000.0 {
                return Err("invalid page size".into());
            }
            if k == 0 {
                if eps {
                    // Writing to String is infallible.
                    let _ = writeln!(out, "%%BoundingBox: 0 0 {:.0} {:.0}\n%%HiResBoundingBox: 0 0 {w:.4} {h:.4}", w.ceil(), h.ceil());
                }
                out.push_str("%%EndComments\n");
            }
            let r = self.renderer.render(RenderRequest { page, scale: (dpi / 72.0) as f32, ..Default::default() });
            if let Some(e) = r.error {
                return Err(format!("page {}: {e}", page + 1));
            }
            let image = encode_jpeg(r.width, r.height, &r.rgba, 100)?;
            if out.len().saturating_add(image.len().saturating_mul(3)) > 512 * 1024 * 1024 {
                return Err("PostScript export exceeds 512 MB; export fewer pages or reduce resolution".into());
            }
            let _ = writeln!(out, "%%Page: {} {}\ngsave", k + 1, k + 1);
            if !eps {
                let _ = writeln!(out, "<< /PageSize [{w:.4} {h:.4}] >> setpagedevice");
            }
            let _ = writeln!(
                out,
                "{w:.4} {h:.4} scale\n/DeviceRGB setcolorspace\n<< /ImageType 1 /Width {} /Height {} /BitsPerComponent 8 /Decode [0 1 0 1 0 1] /ImageMatrix [{} 0 0 -{} 0 {}] /DataSource currentfile /ASCIIHexDecode filter /DCTDecode filter >> image",
                r.width, r.height, r.width, r.height, r.height
            );
            for chunk in image.chunks(40) {
                for b in chunk {
                    let _ = write!(out, "{b:02X}");
                }
                out.push('\n');
            }
            out.push_str(">\ngrestore\n");
            if !eps {
                out.push_str("showpage\n");
            }
            out.push_str("%%PageTrailer\n");
        }
        out.push_str("%%Trailer\n%%EOF\n");
        Ok(out.into_bytes())
    }

    /// Page `page` (0-based) as a PNG at `dpi` (capped by the renderer's size limits).
    pub fn png(&mut self, page: usize, dpi: f64) -> Result<Vec<u8>, String> {
        self.check(page)?;
        let r = self.renderer.render(RenderRequest { page, scale: (dpi.clamp(18.0, 1200.0) / 72.0) as f32, ..Default::default() });
        if let Some(e) = r.error {
            return Err(format!("page {}: {e}", page + 1));
        }
        encode_png(r.width, r.height, &r.rgba)
    }

    /// Page `page` as an image file of `format`.
    pub fn image(&mut self, page: usize, dpi: f64, format: ImageFormat) -> Result<Vec<u8>, String> {
        self.check(page)?;
        let r = self.renderer.render(RenderRequest { page, scale: (dpi.clamp(18.0, 1200.0) / 72.0) as f32, ..Default::default() });
        if let Some(e) = r.error {
            return Err(format!("page {}: {e}", page + 1));
        }
        match format {
            ImageFormat::Png => encode_png(r.width, r.height, &r.rgba),
            ImageFormat::Jpeg { quality } => encode_jpeg(r.width, r.height, &r.rgba, quality),
            ImageFormat::Tiff => encode_tiff(r.width, r.height, &r.rgba),
        }
    }

    /// The reading-order text of a page.
    pub fn text(&mut self, page: usize) -> Result<String, String> {
        self.check(page)?;
        let r = self.renderer.render(RenderRequest { page, kind: RequestKind::Text, scale: 1.0, ..Default::default() });
        match r.text {
            Some(t) => Ok(t.plain_text()),
            None => Err(format!("page {}: {}", page + 1, r.error.unwrap_or_else(|| "no text layer".into()))),
        }
    }

    /// The text of several pages, separated by form feeds (as `pdftotext` does).
    pub fn text_of(&mut self, pages: &[usize]) -> Result<String, String> {
        let mut out = String::new();
        for (k, p) in pages.iter().enumerate() {
            if k > 0 {
                out.push('\u{c}');
            }
            out.push_str(self.text(*p)?.trim_end());
            out.push('\n');
        }
        Ok(out)
    }
}

/// Premultiplied RGBA → PNG (straight alpha).
pub fn encode_png(width: u32, height: u32, premultiplied: &[u8]) -> Result<Vec<u8>, String> {
    let mut rgba = premultiplied.to_vec();
    for px in rgba.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width, height);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().map_err(|e| e.to_string())?;
    w.write_image_data(&rgba).map_err(|e| e.to_string())?;
    w.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

/// Image file formats for Export ▸ Image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Png,
    /// Quality 1–100.
    Jpeg {
        quality: u8,
    },
    Tiff,
}

impl ImageFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg { .. } => "jpg",
            ImageFormat::Tiff => "tif",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg { .. } => "JPEG",
            ImageFormat::Tiff => "TIFF",
        }
    }
}

/// Premultiplied RGBA composited over white → RGB.
fn over_white(premultiplied: &[u8]) -> Vec<u8> {
    premultiplied
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let k = 255 - p[3];
            [p[0].saturating_add(k), p[1].saturating_add(k), p[2].saturating_add(k)]
        })
        .collect()
}

/// Premultiplied RGBA → baseline JPEG (pages are opaque: transparency becomes white paper).
pub fn encode_jpeg(width: u32, height: u32, premultiplied: &[u8], quality: u8) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let rgb = over_white(premultiplied);
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality.clamp(1, 100))
        .write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Premultiplied RGBA → TIFF (RGB, LZW-compressed by the encoder's default).
pub fn encode_tiff(width: u32, height: u32, premultiplied: &[u8]) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    let rgb = over_white(premultiplied);
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::tiff::TiffEncoder::new(&mut out).write_image(&rgb, width, height, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}
