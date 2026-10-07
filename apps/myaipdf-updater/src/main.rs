#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
use std::path::PathBuf;
use std::process::Command;

fn run() -> Result<(), String> {
    if std::env::args_os().len() == 1 {
        return standalone();
    }
    let mut args = std::env::args().skip(1);
    let (mut package, mut application, mut pid) = (None, None, None);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("更新助手参数不完整")?;
        match arg.as_str() {
            "--package" => package = Some(PathBuf::from(value)),
            "--application" => application = Some(PathBuf::from(value)),
            "--wait-pid" => pid = Some(value.parse::<u32>().map_err(|_| "进程编号无效")?),
            _ => return Err("更新助手参数无效".into()),
        }
    }
    let path = package.ok_or("缺少安装包信息")?;
    let application = application.ok_or("缺少安装位置")?;
    let pid = pid.filter(|p| *p > 1 && *p != std::process::id()).ok_or("进程编号无效")?;
    if !path.is_absolute() || !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 65536) {
        return Err("安装包信息文件无效".into());
    }
    let package: printcraft_update::Package =
        serde_json::from_slice(&std::fs::read(&path).map_err(|_| "无法读取更新信息")?).map_err(|_| "更新信息格式无效")?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    loop {
        let status = Command::new("/bin/kill").args(["-0", &pid.to_string()]).status().map_err(|_| "无法确认旧程序已退出")?;
        if !status.success() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            return Err("MyAIPDF 仍在运行，已取消安装。请保存文档并关闭所有窗口后重试。".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    let receipt = match printcraft_update::install(&package, &application) {
        Ok(r) => r,
        Err(e) => {
            if application.exists() {
                let _ = Command::new("/usr/bin/open").arg("-n").arg(&application).status();
            }
            return Err(e);
        }
    };
    // Keep a non-secret receipt beside our instructions, including the recovery backup path.
    if let Some(parent) = path.parent() {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        if let Ok(file) = options.open(parent.join("receipt.json")) {
            let _ = serde_json::to_writer(file, &receipt);
        }
    }
    let status = Command::new("/usr/bin/open").arg("-n").arg(&receipt.application).status().map_err(|_| "更新已安装，请手动打开 MyAIPDF")?;
    if !status.success() {
        return Err("更新已安装，请手动打开 MyAIPDF".into());
    }
    Ok(())
}

fn standalone() -> Result<(), String> {
    let exe = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|_| "无法确定增量安装助手位置")?;
    let bundle = exe.ancestors().find(|p| p.file_name().is_some_and(|n| n == printcraft_update::delta::INSTALLER)).ok_or("请从增量包打开安装助手")?;
    let status =
        Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(bundle).status().map_err(|_| "无法校验增量安装助手")?;
    if !status.success() {
        return Err("增量安装助手签名校验失败，未修改应用".into());
    }
    let directory = bundle.join("Contents/Resources/delta");
    let manifest = printcraft_update::delta::Manifest::read(&directory)?;
    let description = format!(
        "此增量包仅将未改动的 MyAIPDF {} 升级到 {}。\n\n请先保存 PDF 并退出旧程序，然后选择要升级的 MyAIPDF.app。安装会保留旧程序备份和个人设置，不下载完整包。",
        manifest.from_version, manifest.version
    );
    if rfd::MessageDialog::new().set_title("MyAIPDF 增量更新").set_description(description).set_buttons(rfd::MessageButtons::OkCancel).show()
        != rfd::MessageDialogResult::Ok
    {
        return Ok(());
    }
    let Some(application) = rfd::FileDialog::new()
        .set_title(format!("选择 {} 版 MyAIPDF.app", manifest.from_version))
        .set_directory("/Applications")
        .add_filter("Mac 应用", &["app"])
        .pick_file()
    else {
        return Ok(());
    };
    let receipt = printcraft_update::install_delta_directory(&directory, &application)?;
    let description = format!(
        "已更新到 {}。\n旧程序备份：{}\n个人设置未变动。是否打开新版？",
        manifest.version,
        receipt.backup.as_ref().map_or_else(|| "无".into(), |p| p.display().to_string())
    );
    if rfd::MessageDialog::new().set_title("MyAIPDF 更新完成").set_description(description).set_buttons(rfd::MessageButtons::YesNo).show()
        == rfd::MessageDialogResult::Yes
    {
        Command::new("/usr/bin/open").arg("-n").arg(&receipt.application).status().map_err(|_| "更新成功，请手动打开 MyAIPDF")?;
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        rfd::MessageDialog::new().set_title("MyAIPDF 更新").set_description(error).set_level(rfd::MessageLevel::Error).show();
        std::process::exit(1);
    }
}
