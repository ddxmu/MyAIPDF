//! Verified, recoverable bundle replacement. No privileged commands or security overrides.
use super::{BUNDLE_ID, Installed, Package};
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(command: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    if Command::new(command).args(args).output().map_err(|_| format!("无法运行更新步骤：{command}"))?.status.success() {
        Ok(())
    } else {
        Err(format!("更新步骤未通过：{command}。旧程序与个人设置未被删除。"))
    }
}

fn property(app: &Path, name: &str) -> Result<String, String> {
    let r = Command::new("/usr/bin/plutil")
        .args(["-extract", name, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .map_err(|_| "无法读取应用信息")?;
    if !r.status.success() {
        return Err("更新包的应用信息无效".into());
    }
    String::from_utf8(r.stdout).map(|s| s.trim().to_owned()).map_err(|_| "应用信息编码无效".into())
}

fn check_bundle(app: &Path, version: Option<&str>) -> Result<(), String> {
    if !std::fs::symlink_metadata(app).is_ok_and(|m| m.is_dir())
        || property(app, "CFBundleIdentifier")? != BUNDLE_ID
        || property(app, "CFBundleExecutable")? != "MyAIPDF"
    {
        return Err("更新包不是 MyAIPDF 应用，未安装".into());
    }
    if let Some(version) = version
        && property(app, "CFBundleShortVersionString")? != version
    {
        return Err("更新包版本与 GitHub 发布记录不一致，未安装".into());
    }
    run("/usr/bin/codesign", &["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), app.as_os_str()])
}

pub(super) fn install(package: &Package, application: &Path) -> Result<Installed, String> {
    if application.file_name().and_then(|s| s.to_str()) != Some("MyAIPDF.app") || !application.is_absolute() {
        return Err("安装位置必须是绝对路径下的 MyAIPDF.app".into());
    }
    let parent = application.parent().ok_or("安装位置无效")?.canonicalize().map_err(|_| "安装目录不存在或无权限")?;
    let target = parent.join("MyAIPDF.app");
    if target.exists() {
        check_bundle(&target, None)?;
        let running =
            Command::new("/usr/sbin/lsof").arg("-t").arg(target.join("Contents/MacOS/MyAIPDF")).output().map_err(|_| "无法确认旧程序已退出")?;
        if running.status.success() || !running.stdout.is_empty() {
            return Err("请先关闭所有运行中的 MyAIPDF 窗口，然后重试安装".into());
        }
        if running.status.code() != Some(1) {
            return Err("无法确认旧程序已退出，未替换应用".into());
        }
    }
    let work = super::private_directory(&std::env::temp_dir())?;
    let mount = work.join("volume");
    std::fs::create_dir(&mount).map_err(|_| "无法创建更新装载目录")?;
    run(
        "/usr/bin/hdiutil",
        &["attach".as_ref(), "-readonly".as_ref(), "-nobrowse".as_ref(), "-mountpoint".as_ref(), mount.as_os_str(), package.path.as_os_str()],
    )?;
    let outcome = activate(&mount, &parent, &target, &package.version);
    // Eject our exact private mount even when validation/copying failed.
    let detach = run("/usr/bin/hdiutil", &["detach".as_ref(), mount.as_os_str()]);
    match outcome {
        Ok(receipt) => {
            let _ = detach;
            Ok(receipt)
        }
        Err(e) => Err(e),
    }
}

fn activate(mount: &Path, parent: &Path, target: &Path, version: &str) -> Result<Installed, String> {
    let source = mount.join("MyAIPDF.app");
    check_bundle(&source, Some(version))?;
    let nonce = getrandom::u64().map_err(|_| "无法准备应用更新")?;
    let staged = parent.join(format!(".MyAIPDF-staged-{nonce:016x}.app"));
    let backup = parent.join(format!(".MyAIPDF-backup-{nonce:016x}.app"));
    if staged.exists() || backup.exists() {
        return Err("更新缓存位置已存在，请重试".into());
    }
    std::fs::create_dir(&staged).map_err(|_| "应用目录不可写，请手动将安装包中的 MyAIPDF 拖入应用程序")?;
    run("/usr/bin/ditto", &[source.as_os_str(), staged.as_os_str()])?;
    check_bundle(&staged, Some(version))?;
    replace_staged(&staged, target, &backup)
}

/// Same-directory renames: preserve the original until the new verified bundle is ready.
pub(super) fn replace_staged(staged: &Path, target: &Path, backup: &Path) -> Result<Installed, String> {
    let had_old = target.exists();
    if had_old {
        std::fs::rename(target, backup).map_err(|_| "无法备份旧程序；旧程序未变动")?;
    }
    if let Err(e) = std::fs::rename(staged, target) {
        if had_old && std::fs::rename(backup, target).is_err() {
            return Err(format!("安装失败；旧程序完整保留在 {}，请将其移回 {}。原因：{e}", backup.display(), target.display()));
        }
        return Err("安装失败，旧程序已恢复。个人设置未变动。".into());
    }
    Ok(Installed { application: target.to_path_buf(), backup: had_old.then(|| PathBuf::from(backup)) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_preserves_backup_and_failed_activation_restores_old_bundle() {
        let directory = crate::private_directory(&std::env::temp_dir()).unwrap();
        let target = directory.join("MyAIPDF.app");
        let staged = directory.join("staged.app");
        let backup = directory.join("backup.app");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("version"), "old").unwrap();
        assert!(replace_staged(&staged, &target, &backup).is_err());
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "old");
        assert!(!backup.exists());
        std::fs::create_dir(&staged).unwrap();
        std::fs::write(staged.join("version"), "new").unwrap();
        let installed = replace_staged(&staged, &target, &backup).unwrap();
        assert_eq!(installed.backup, Some(backup.clone()));
        assert_eq!(std::fs::read_to_string(target.join("version")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(backup.join("version")).unwrap(), "old");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
