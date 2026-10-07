//! Render the original MyAIPDF vector; system iconutil assembles the generated sizes.
use anyhow::{Context, Result, ensure};

pub fn run(_: &[String]) -> Result<()> {
    let root = crate::gates::root();
    let svg = std::fs::read(root.join("assets/myaipdf/icon.svg"))?;
    let tree = resvg::usvg::Tree::from_data(&svg, &resvg::usvg::Options::default())?;
    let dir = root.join("target/myaipdf-icon/MyAIPDF.iconset");
    std::fs::create_dir_all(&dir)?;
    for logical in [16, 32, 128, 256, 512] {
        for scale in [1, 2] {
            let size = logical * scale;
            let mut pixmap = resvg::tiny_skia::Pixmap::new(size, size).context("allocate icon")?;
            resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(size as f32 / 1024.0, size as f32 / 1024.0), &mut pixmap.as_mut());
            let suffix = if scale == 2 { "@2x" } else { "" };
            pixmap.save_png(dir.join(format!("icon_{logical}x{logical}{suffix}.png")))?;
            if size == 1024 {
                pixmap.save_png(root.join("assets/myaipdf/icon-1024.png"))?;
            }
        }
    }
    let result = std::process::Command::new("/usr/bin/iconutil")
        .args(["-c", "icns"])
        .arg(&dir)
        .arg("-o")
        .arg(root.join("assets/myaipdf/MyAIPDF.icns"))
        .status()?;
    ensure!(result.success(), "iconutil failed");
    Ok(())
}
