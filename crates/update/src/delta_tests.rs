//! Synthetic signed bundles, never the user's installed application or settings.
use crate::delta::*;

#[test]
fn paths_sizes_and_missing_data_are_rejected() {
    for path in ["/Contents/a", "Contents/../a", "Contents//a", "Contents/a\\b", "Contents/./a", "Other/a"] {
        assert!(!valid_path(path));
    }
    assert!(valid_path("Contents/Resources/使用说明.txt"));
    let hash = Fingerprint { sha256: "a".repeat(64), size: 4, executable: false };
    let mut m = Manifest {
        format: 1,
        from_version: "0.1.2".into(),
        version: "0.1.3".into(),
        architecture: "aarch64".into(),
        files: vec![File { path: "Contents/a".into(), before: Some(hash.clone()), after: Some(hash), payload: None }],
    };
    m.validate().unwrap();
    m.files[0].after.as_mut().unwrap().size = 5;
    assert!(m.validate().is_err());
    m.files[0].after = None;
    m.validate().unwrap();
    m.files.push(m.files[0].clone());
    assert!(m.validate().is_err());
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod mac {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    struct Fixture {
        root: PathBuf,
        old: PathBuf,
        new: PathBuf,
        delta: PathBuf,
        manifest: Manifest,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn sign(app: &Path) {
        assert!(Command::new("/usr/bin/codesign").args(["--force", "--sign", "-", "--timestamp=none"]).arg(app).output().unwrap().status.success());
    }
    fn fixture() -> Fixture {
        let root = crate::private_directory(&std::env::temp_dir()).unwrap();
        let old = root.join("MyAIPDF.app");
        let new = root.join("expected.app");
        let delta = root.join("delta");
        for (app, version, fill) in [(&old, "0.1.2", b'a'), (&new, "0.1.3", b'b')] {
            std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
            std::fs::create_dir_all(app.join("Contents/Resources")).unwrap();
            std::fs::copy(std::env::current_exe().unwrap(), app.join("Contents/MacOS/MyAIPDF")).unwrap();
            std::fs::write(app.join("Contents/Info.plist"), format!("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>local.myaipdf.desktop</string><key>CFBundleExecutable</key><string>MyAIPDF</string><key>CFBundleShortVersionString</key><string>{version}</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>")).unwrap();
            std::fs::write(app.join("Contents/Resources/change"), vec![fill; 4096]).unwrap();
            std::fs::write(app.join("Contents/Resources/model"), vec![7; 8192]).unwrap();
            sign(app);
        }
        std::fs::write(root.join("personal-settings.txt"), "untouched").unwrap();
        std::fs::create_dir(&delta).unwrap();
        let (before, after) = (snapshot(&old).unwrap(), snapshot(&new).unwrap());
        let mut manifest =
            Manifest { format: 1, from_version: "0.1.2".into(), version: "0.1.3".into(), architecture: "aarch64".into(), files: Vec::new() };
        let paths: std::collections::BTreeSet<_> = before.keys().chain(after.keys()).collect();
        for (i, path) in paths.into_iter().enumerate() {
            let (a, b) = (before.get(path).cloned(), after.get(path).cloned());
            let payload = if a != b {
                let mut bytes = std::fs::read(new.join(path)).unwrap();
                let kind = if path.ends_with("/change") {
                    let mut patch = Vec::new();
                    qbsdiff::Bsdiff::new(&std::fs::read(old.join(path)).unwrap(), &bytes).compare(std::io::Cursor::new(&mut patch)).unwrap();
                    bytes = patch;
                    Kind::Bsdiff
                } else {
                    Kind::Copy
                };
                let file = format!("{i:04}.bin");
                std::fs::write(delta.join(&file), bytes).unwrap();
                let h = fingerprint(&delta.join(&file)).unwrap();
                Some(Payload { file, kind, sha256: h.sha256, size: h.size })
            } else {
                None
            };
            manifest.files.push(File { path: path.clone(), before: a, after: b, payload });
        }
        std::fs::write(delta.join("manifest.json"), serde_json::to_vec(&manifest).unwrap()).unwrap();
        Fixture { root, old, new, delta, manifest }
    }

    #[test]
    fn binary_delta_reconstructs_signed_app_and_keeps_models_settings_and_backup() {
        let f = fixture();
        let original = snapshot(&f.old).unwrap();
        assert!(f.manifest.files.iter().find(|e| e.path.ends_with("/model")).unwrap().payload.is_none());
        let receipt = crate::install_delta_directory(&f.delta, &f.old).unwrap();
        assert_eq!(snapshot(&f.old).unwrap(), snapshot(&f.new).unwrap());
        assert_eq!(snapshot(receipt.backup.as_ref().unwrap()).unwrap(), original);
        assert_eq!(std::fs::read_to_string(f.root.join("personal-settings.txt")).unwrap(), "untouched");
        assert!(crate::install_delta_directory(&f.delta, &f.old).unwrap_err().contains("需要未改动的"));
    }

    #[test]
    fn altered_base_corrupt_payload_bad_output_and_open_executable_never_replace_app() {
        let f = fixture();
        let original = snapshot(&f.old).unwrap();
        let held = std::fs::File::open(f.old.join("Contents/MacOS/MyAIPDF")).unwrap();
        assert!(crate::install_delta_directory(&f.delta, &f.old).unwrap_err().contains("退出全部"));
        drop(held);
        let payload = f.manifest.files.iter().find_map(|e| e.payload.as_ref()).unwrap();
        let bytes = std::fs::read(f.delta.join(&payload.file)).unwrap();
        std::fs::write(f.delta.join(&payload.file), b"corrupt").unwrap();
        assert!(crate::install_delta_directory(&f.delta, &f.old).unwrap_err().contains("SHA-256"));
        assert_eq!(snapshot(&f.old).unwrap(), original);
        std::fs::write(f.delta.join(&payload.file), bytes).unwrap();
        let mut wrong = f.manifest.clone();
        let code_resources = wrong.files.iter_mut().find(|e| e.path.ends_with("/_CodeSignature/CodeResources")).unwrap();
        let data = code_resources.payload.as_mut().unwrap();
        std::fs::write(f.delta.join(&data.file), b"invalid signature resource").unwrap();
        let hash = fingerprint(&f.delta.join(&data.file)).unwrap();
        data.sha256 = hash.sha256.clone();
        data.size = hash.size;
        code_resources.after = Some(hash);
        std::fs::write(f.delta.join("manifest.json"), serde_json::to_vec(&wrong).unwrap()).unwrap();
        assert!(crate::install_delta_directory(&f.delta, &f.old).unwrap_err().contains("codesign"));
        assert_eq!(snapshot(&f.old).unwrap(), original);
        std::fs::write(f.delta.join("manifest.json"), serde_json::to_vec(&f.manifest).unwrap()).unwrap();
        std::fs::write(f.old.join("Contents/Resources/change"), b"changed base").unwrap();
        let changed = snapshot(&f.old).unwrap();
        assert!(crate::install_delta_directory(&f.delta, &f.old).is_err());
        assert_eq!(snapshot(&f.old).unwrap(), changed);
    }

    #[test]
    fn payload_links_undeclared_files_and_corrupt_bsdiff_headers_are_rejected() {
        let f = fixture();
        std::fs::write(f.delta.join("extra"), b"unlisted").unwrap();
        assert!(f.manifest.verify_payloads(&f.delta).is_err());
        std::fs::remove_file(f.delta.join("extra")).unwrap();
        let p = f.manifest.files.iter().find_map(|e| e.payload.as_ref().filter(|p| matches!(p.kind, Kind::Bsdiff))).unwrap();
        let actual = f.delta.join(&p.file);
        let backup = f.root.join("kept-payload");
        std::fs::rename(&actual, &backup).unwrap();
        std::os::unix::fs::symlink(&backup, &actual).unwrap();
        assert!(f.manifest.verify_payloads(&f.delta).is_err());
        std::fs::remove_file(&actual).unwrap();
        let mut bad = std::fs::read(&backup).unwrap();
        bad[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
        std::fs::write(&actual, bad).unwrap();
        let mut m = f.manifest.clone();
        // Update the manifest hash too: malformed patch headers must be rejected after hashing.
        let info = fingerprint(&actual).unwrap();
        let entry = m.files.iter_mut().find(|e| e.payload.as_ref().is_some_and(|v| v.file == p.file)).unwrap();
        let payload = entry.payload.as_mut().unwrap();
        payload.sha256 = info.sha256;
        payload.size = info.size;
        std::fs::write(f.delta.join("manifest.json"), serde_json::to_vec(&m).unwrap()).unwrap();
        let original = snapshot(&f.old).unwrap();
        assert!(crate::install_delta_directory(&f.delta, &f.old).unwrap_err().contains("增量头"));
        assert_eq!(snapshot(&f.old).unwrap(), original);
    }
}
