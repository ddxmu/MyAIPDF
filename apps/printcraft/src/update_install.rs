//! Launch only the packaged, signed helper. It waits for the GUI to exit; it never kills it.
use printcraft_update::Package;
use std::path::Path;

pub fn start(package: &Package) -> Result<(), String> {
    printcraft_update::verify_package(package)?;
    let executable = std::env::current_exe().and_then(|p| p.canonicalize()).map_err(|_| "无法确定当前应用位置")?;
    let bundle = executable
        .ancestors()
        .find(|p| p.file_name().is_some_and(|n| n == "MyAIPDF.app"))
        .ok_or("请先将 MyAIPDF.app 拖入应用程序文件夹，再使用自动安装")?;
    let target = if bundle.starts_with("/Volumes") { Path::new("/Applications/MyAIPDF.app") } else { bundle };
    let directory = printcraft_update::private_directory(&std::env::temp_dir())?;
    let helper = directory.join("myaipdf-updater");
    let mut helper_file = std::fs::OpenOptions::new();
    helper_file.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        helper_file.mode(0o700);
    }
    let mut output = helper_file.open(&helper).map_err(|_| "无法准备更新助手")?;
    let mut source =
        std::fs::File::open(bundle.join("Contents/MacOS/myaipdf-updater")).map_err(|_| "应用缺少更新助手，请使用对应版本增量包内的安装助手")?;
    std::io::copy(&mut source, &mut output).map_err(|_| "无法复制更新助手")?;
    output.sync_all().map_err(|_| "无法保存更新助手")?;
    drop(output);
    let instructions = directory.join("package.json");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&instructions).map_err(|_| "无法保存更新信息")?;
    serde_json::to_writer(&mut file, package).map_err(|_| "无法保存更新信息")?;
    file.sync_all().map_err(|_| "无法保存更新信息")?;
    std::process::Command::new(helper)
        .arg("--package")
        .arg(instructions)
        .arg("--application")
        .arg(target)
        .arg("--wait-pid")
        .arg(std::process::id().to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| "无法启动更新助手，应用未退出，请重试")?;
    Ok(())
}
