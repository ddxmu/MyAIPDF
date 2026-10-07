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

pub(super) fn activate(mount: &Path, parent: &Path, target: &Path, version: &str) -> Result<Installed, String> {
    let source = mount.join("MyAIPDF.app");
    if std::fs::symlink_metadata(&source).is_ok() {
        check_bundle(&source, Some(version))?;
        let nonce = getrandom::u64().map_err(|_| "无法准备应用更新")?;
        let staged = parent.join(format!(".MyAIPDF-staged-{nonce:016x}.app"));
        let backup = parent.join(format!(".MyAIPDF-backup-{nonce:016x}.app"));
        if staged.exists() || backup.exists() {
            return Err("更新缓存位置已存在，请重试".into());
        }
        std::fs::create_dir(&staged).map_err(|_| "应用目录不可写，请手动将安装包中的 MyAIPDF 拖入应用程序")?;
        let outcome = (|| {
            run("/usr/bin/ditto", &[source.as_os_str(), staged.as_os_str()])?;
            check_bundle(&staged, Some(version))?;
            if target.exists() {
                check_bundle(target, None)?;
                closed(target)?;
            }
            replace_staged(&staged, target, &backup)
        })();
        if outcome.is_err() {
            let _ = std::fs::remove_dir_all(&staged);
        }
        return outcome;
    }
    let installer = mount.join(super::delta::INSTALLER);
    if property(&installer, "CFBundleIdentifier")? != "local.myaipdf.delta" || property(&installer, "CFBundleShortVersionString")? != version {
        return Err("增量安装助手的标识或版本无效".into());
    }
    run("/usr/bin/codesign", &["--verify".as_ref(), "--deep".as_ref(), "--strict".as_ref(), installer.as_os_str()])?;
    install_directory(&installer.join("Contents/Resources/delta"), target, Some(version))
}

fn closed(target: &Path) -> Result<(), String> {
    let r = Command::new("/usr/sbin/lsof").arg("-t").arg(target.join("Contents/MacOS/MyAIPDF")).output().map_err(|_| "无法确认旧程序已退出")?;
    if r.status.success() || !r.stdout.is_empty() {
        return Err("请先保存 PDF 并退出全部 MyAIPDF 窗口，再安装更新".into());
    }
    if r.status.code() != Some(1) {
        return Err("无法确认旧程序已退出，未替换应用".into());
    }
    Ok(())
}

pub(super) fn install_directory(directory: &Path, application: &Path, version: Option<&str>) -> Result<Installed, String> {
    use super::delta::{Kind, Manifest, snapshot};
    if !cfg!(target_arch = "aarch64") {
        return Err("此增量包仅适用于 Apple Silicon Mac".into());
    }
    if application.file_name().and_then(|s| s.to_str()) != Some("MyAIPDF.app")
        || !application.is_absolute()
        || !std::fs::symlink_metadata(directory).is_ok_and(|m| m.is_dir())
    {
        return Err("必须选择现有的 MyAIPDF.app，增量数据目录不能是链接".into());
    }
    let parent = application.parent().ok_or("安装位置无效")?.canonicalize().map_err(|_| "应用目录不可访问")?;
    let target = parent.join("MyAIPDF.app");
    let manifest = Manifest::read(directory)?;
    if version.is_some_and(|v| v != manifest.version) {
        return Err("增量版本与发布记录不一致".into());
    }
    check_bundle(&target, Some(&manifest.from_version))
        .map_err(|_| format!("此增量包需要未改动的 {} 版 MyAIPDF；未下载完整包，旧程序未变动。", manifest.from_version))?;
    closed(&target)?;
    let before = manifest.expected(false);
    if snapshot(&target)? != before {
        return Err("基础版本文件校验不一致，不能应用此增量包；旧程序未变动".into());
    }
    manifest.verify_payloads(directory)?;
    let nonce = getrandom::u64().map_err(|_| "无法准备增量更新")?;
    let staged = parent.join(format!(".MyAIPDF-staged-{nonce:016x}.app"));
    let backup = parent.join(format!(".MyAIPDF-backup-{nonce:016x}.app"));
    if staged.exists() || backup.exists() {
        return Err("更新暂存位置已存在，请重试".into());
    }
    std::fs::create_dir(&staged).map_err(|_| "应用目录不可写，请选择你有权限的 MyAIPDF.app")?;
    let work = super::private_directory(&std::env::temp_dir())?;
    let outcome = (|| {
        run("/usr/bin/ditto", &[target.as_os_str(), staged.as_os_str()])?;
        for (index, file) in manifest.files.iter().enumerate() {
            let output = staged.join(&file.path);
            if file.after.is_none() {
                std::fs::remove_file(&output).map_err(|_| "无法移除旧版的已废弃文件")?;
                continue;
            }
            if let Some(payload) = &file.payload {
                let source = directory.join(&payload.file);
                let temp = work.join(format!("patched-{index}"));
                match payload.kind {
                    Kind::Copy => {
                        std::fs::copy(source, &temp).map_err(|_| "无法写入新增文件")?;
                    }
                    Kind::Bsdiff => apply_patch_file(&output, &temp, &source, file.after.as_ref().ok_or("增量缺少目标信息")?.size)?,
                }
                let folder = output.parent().ok_or("增量目标位置无效")?;
                std::fs::create_dir_all(folder).map_err(|_| "无法创建增量目录")?;
                std::fs::rename(&temp, &output).map_err(|_| "无法写入增量结果")?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let executable = file.after.as_ref().is_some_and(|f| f.executable);
                    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(if executable { 0o755 } else { 0o644 }))
                        .map_err(|_| "无法设置应用文件权限")?;
                }
            }
        }
        if snapshot(&staged)? != manifest.expected(true) {
            return Err("增量合成后的文件校验失败，旧程序未变动".into());
        }
        check_bundle(&staged, Some(&manifest.version))?;
        // Recheck the base and running state immediately before the recoverable swap.
        closed(&target)?;
        if snapshot(&target)? != before {
            return Err("旧程序在准备期间发生变化，已停止更新".into());
        }
        replace_staged(&staged, &target, &backup)
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_dir_all(&staged);
    }
    let _ = std::fs::remove_dir_all(work);
    outcome
}

fn apply_patch_file(old: &Path, output: &Path, patch: &Path, expected_size: u64) -> Result<(), String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut header = [0u8; 32];
    std::fs::File::open(patch).and_then(|mut f| f.read_exact(&mut header)).map_err(|_| "增量二进制头不完整")?;
    if header.get(..8) != Some(b"BSDIFF40") {
        return Err("增量二进制格式无效".into());
    }
    let number = |offset| -> Result<u64, String> {
        let b = header.get(offset..offset + 8).ok_or("增量头无效")?;
        if b.get(7).is_none_or(|v| v & 0x80 != 0) {
            return Err("增量头包含负长度".into());
        }
        let array: [u8; 8] = b.try_into().map_err(|_| "增量头无效")?;
        Ok(u64::from_le_bytes(array))
    };
    let length = std::fs::metadata(patch).map_err(|_| "无法读取增量大小")?.len();
    if number(24)? != expected_size || number(8)?.checked_add(number(16)?).and_then(|n| n.checked_add(32)).is_none_or(|n| n > length) {
        return Err("增量二进制长度与清单不一致".into());
    }
    let mut child = Command::new("/usr/bin/bspatch")
        .arg(old)
        .arg(output)
        .arg(patch)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "无法运行系统增量合成工具")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
    loop {
        if let Some(status) = child.try_wait().map_err(|_| "无法检查增量合成状态")? {
            return if status.success() { Ok(()) } else { Err("增量合成未通过，旧程序未变动".into()) };
        }
        if std::time::Instant::now() >= deadline || std::fs::metadata(output).is_ok_and(|m| m.len() > expected_size) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("增量合成超时或超过大小限制，旧程序未变动".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
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
