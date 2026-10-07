//! Release-only binary diff generation. The product applies BSDIFF40 with macOS bspatch.
use anyhow::{Context, bail};
use printcraft_update::delta::{File, Kind, Manifest, Payload, fingerprint, snapshot};
use std::collections::BTreeSet;
use std::path::Path;

fn version(app: &Path) -> anyhow::Result<String> {
    let r = std::process::Command::new("/usr/bin/plutil")
        .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()?;
    if !r.status.success() {
        bail!("not a macOS application bundle");
    }
    Ok(String::from_utf8(r.stdout)?.trim().to_owned())
}

pub fn run(args: &[String]) -> anyhow::Result<()> {
    let [old, new, out] = args else {
        bail!("usage: cargo xtask myaipdf-delta <old.app> <new.app> <new-delta-directory>");
    };
    let (old, new, out) = (Path::new(old), Path::new(new), Path::new(out));
    if out.exists() || out.is_symlink() {
        bail!("refusing to replace an existing delta output");
    }
    let (before, after) = (snapshot(old).map_err(anyhow::Error::msg)?, snapshot(new).map_err(anyhow::Error::msg)?);
    let mut manifest = Manifest { format: 1, from_version: version(old)?, version: version(new)?, architecture: "aarch64".into(), files: Vec::new() };
    std::fs::create_dir(out)?;
    let paths: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    for (index, path) in paths.into_iter().enumerate() {
        let (source, target) = (before.get(path).cloned(), after.get(path).cloned());
        let payload = if target.is_some() && source != target {
            let target_bytes = std::fs::read(new.join(path)).with_context(|| format!("reading {path}"))?;
            let mut bytes = Vec::new();
            let mut kind = Kind::Copy;
            if source.is_some() && target_bytes.len() >= 4096 {
                let source_bytes = std::fs::read(old.join(path))?;
                qbsdiff::Bsdiff::new(&source_bytes, &target_bytes).compare(std::io::Cursor::new(&mut bytes))?;
                if bytes.len() < target_bytes.len() {
                    kind = Kind::Bsdiff;
                } else {
                    bytes = target_bytes;
                }
            } else {
                bytes = target_bytes;
            }
            let name = format!("{index:04}.{}", if matches!(kind, Kind::Bsdiff) { "bsdiff" } else { "bin" });
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().write(true).create_new(true).open(out.join(&name))?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            let hash = fingerprint(&out.join(&name)).map_err(anyhow::Error::msg)?;
            println!("delta: {path}: {} bytes ({kind:?})", hash.size);
            Some(Payload { file: name, kind, sha256: hash.sha256, size: hash.size })
        } else {
            None
        };
        manifest.files.push(File { path: path.clone(), before: source, after: target, payload });
    }
    manifest.validate().map_err(anyhow::Error::msg)?;
    let file = std::fs::OpenOptions::new().write(true).create_new(true).open(out.join("manifest.json"))?;
    serde_json::to_writer_pretty(file, &manifest)?;
    manifest.verify_payloads(out).map_err(anyhow::Error::msg)?;
    println!(
        "delta: {} -> {}, {} changed files",
        manifest.from_version,
        manifest.version,
        manifest.files.iter().filter(|f| f.payload.is_some() || f.after.is_none()).count()
    );
    Ok(())
}
