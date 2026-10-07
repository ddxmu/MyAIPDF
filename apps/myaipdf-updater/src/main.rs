#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
use std::path::PathBuf;
use std::process::Command;

fn run() -> Result<(), String> {
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

fn main() {
    if let Err(error) = run() {
        rfd::MessageDialog::new().set_title("MyAIPDF 更新").set_description(error).set_level(rfd::MessageLevel::Error).show();
        std::process::exit(1);
    }
}
