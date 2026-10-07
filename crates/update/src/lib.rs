//! User-initiated MyAIPDF updates. No startup checks, telemetry, or executable release scripts.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};

pub mod delta;
#[cfg(test)]
mod delta_tests;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(test)]
mod tests;

pub const APP_VERSION: &str = "0.1.3";
pub const REPOSITORY: &str = "https://github.com/ddxmu/MyAIPDF";
pub const RELEASES_PAGE: &str = "https://github.com/ddxmu/MyAIPDF/releases";
pub const LATEST_API: &str = "https://api.github.com/repos/ddxmu/MyAIPDF/releases/latest";
pub const BUNDLE_ID: &str = "local.myaipdf.desktop";
pub const MAX_PACKAGE_BYTES: u64 = 300 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub version: String,
    pub url: String,
    pub notes: String,
    pub asset: Option<Asset>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package {
    pub path: PathBuf,
    pub version: String,
    pub sha256: String,
    pub size: u64,
}

pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse_version(latest), parse_version(current)), (Some(l), Some(c)) if l > c)
}

fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let mut next = |required: bool| match parts.next() {
        Some(p) => p.parse::<u64>().ok(),
        None if required => None,
        None => Some(0),
    };
    let version = (next(true)?, next(false)?, next(false)?);
    parts.next().is_none().then_some(version)
}

fn valid_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

pub fn parse_release(body: &str) -> Result<Release, String> {
    parse_release_for(body, APP_VERSION)
}

/// Select only a delta built for the caller's exact base; never fall back to a full DMG.
pub fn parse_release_for(body: &str, current: &str) -> Result<Release, String> {
    if body.len() > 1 << 20 {
        return Err("GitHub 发布信息过大".into());
    }
    let v: Value = serde_json::from_str(body).map_err(|_| "GitHub 发布信息不是有效 JSON")?;
    if v["draft"].as_bool() != Some(false) || v["prerelease"].as_bool() != Some(false) {
        return Err("只安装正式发布版本，不安装草稿或预发布版本".into());
    }
    let tag = v["tag_name"].as_str().ok_or("GitHub 未返回版本号")?;
    if tag.len() > 64 || !tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_')) || parse_version(tag).is_none() {
        return Err("GitHub 版本号无效".into());
    }
    let release_url = format!("{RELEASES_PAGE}/tag/{tag}");
    if v["html_url"].as_str() != Some(release_url.as_str()) {
        return Err("发布页不属于 ddxmu/MyAIPDF，已拒绝".into());
    }
    let assets = v["assets"].as_array().ok_or("GitHub 未返回安装包列表")?;
    let version = tag.trim_start_matches(['v', 'V']);
    let filename = delta_filename(version, current);
    let found: Vec<_> = assets.iter().filter(|a| a["name"].as_str() == Some(filename.as_str())).collect();
    if found.len() > 1 {
        return Err("GitHub 返回了重复的安装包".into());
    }
    let asset = match found.first() {
        None => None,
        Some(a) => {
            let url = format!("{RELEASES_PAGE}/download/{tag}/{filename}");
            if a["browser_download_url"].as_str() != Some(url.as_str()) || a["state"].as_str() != Some("uploaded") {
                return Err("安装包地址或上传状态无效".into());
            }
            let size = a["size"].as_u64().filter(|n| *n > 0 && *n <= MAX_PACKAGE_BYTES).ok_or("安装包大小无效或超过 300 MB")?;
            let digest = a["digest"].as_str().and_then(|s| s.strip_prefix("sha256:")).filter(|s| valid_digest(s));
            // Older releases without a digest remain inspectable, but cannot auto-install.
            digest.map(|digest| Asset { url, size, sha256: digest.to_ascii_lowercase() })
        }
    };
    Ok(Release {
        version: tag.trim_start_matches(['v', 'V']).to_owned(),
        url: release_url,
        notes: v["body"].as_str().unwrap_or_default().chars().take(8000).collect(),
        asset,
    })
}

pub fn delta_filename(version: &str, base: &str) -> String {
    format!("MyAIPDF-{version}-from-{base}.delta.dmg")
}

#[cfg(not(target_arch = "wasm32"))]
fn agent() -> ureq::Agent {
    let native = rustls_native_certs::load_native_certs();
    let certs = native.certs.iter().map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned()).collect::<Vec<_>>();
    ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(180)))
        .max_redirects(0)
        .http_status_as_error(false)
        .tls_config(ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::new_with_certs(&certs)).build())
        .build()
        .new_agent()
}

#[cfg(not(target_arch = "wasm32"))]
fn check_url(url: &str) -> Result<Release, String> {
    let mut r = agent()
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", "MyAIPDF")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .call()
        .map_err(|_| "无法连接 GitHub，请检查网络后重试")?;
    if !r.status().is_success() {
        return Err(match r.status().as_u16() {
            404 => "GitHub 尚未发布正式版本",
            403 | 429 => "GitHub 请求受限，请稍后重试",
            _ => "GitHub 未能返回最新版本",
        }
        .into());
    }
    let body = r.body_mut().with_config().limit(1 << 20).read_to_string().map_err(|_| "无法读取 GitHub 发布信息")?;
    parse_release(&body)
}

pub fn check_latest() -> Result<Release, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        check_url(LATEST_API)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Err("请使用 macOS 桌面版检查和安装更新".into())
    }
}

/// Unique private directory; never replaces a pre-existing file or directory.
pub fn private_directory(parent: &Path) -> Result<PathBuf, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let nonce = getrandom::u64().map_err(|_| "无法创建更新缓存")?;
        let dir = parent.join(format!("myaipdf-update-{nonce:016x}"));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).map_err(|_| "无法创建更新目录；请检查磁盘空间与写入权限")?;
        Ok(dir)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = parent;
        Err("浏览器不能安装桌面更新".into())
    }
}

pub fn verify_package(package: &Package) -> Result<(), String> {
    if !valid_digest(&package.sha256) || parse_version(&package.version).is_none() || package.size == 0 || package.size > MAX_PACKAGE_BYTES {
        return Err("更新安装包的校验信息无效".into());
    }
    let metadata = std::fs::symlink_metadata(&package.path).map_err(|_| "更新安装包不存在")?;
    if !metadata.is_file() || metadata.len() != package.size {
        return Err("更新安装包不完整或不是普通文件，未安装".into());
    }
    let mut file = std::fs::File::open(&package.path).map_err(|_| "无法读取更新安装包")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer).map_err(|_| "无法读取更新安装包")?;
        if n == 0 {
            break;
        }
        bytes = bytes.checked_add(n as u64).filter(|n| *n <= package.size).ok_or("安装包读取大小发生变化，未安装")?;
        hash.update(buffer.get(..n).ok_or("读取安装包失败")?);
    }
    let actual = hash.finalize().iter().map(|b| format!("{b:02x}")).collect::<String>();
    if bytes != package.size || actual != package.sha256.to_ascii_lowercase() {
        return Err("安装包 SHA-256 校验不一致，已拒绝安装；请重新下载".into());
    }
    Ok(())
}

pub fn download(release: &Release, directory: &Path, progress: impl Fn(u64, u64)) -> Result<Package, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        let asset = release.asset.as_ref().ok_or("此版本缺少可校验的 Mac 安装包，请查看 GitHub 发布页")?;
        let filename = delta_filename(&release.version, APP_VERSION);
        let expected = format!("{RELEASES_PAGE}/download/v{}/{filename}", release.version);
        let expected_without_v = format!("{RELEASES_PAGE}/download/{}/{filename}", release.version);
        if asset.url != expected && asset.url != expected_without_v {
            return Err("安装包地址不属于指定仓库".into());
        }
        if asset.size == 0 || asset.size > MAX_PACKAGE_BYTES || !valid_digest(&asset.sha256) {
            return Err("安装包校验信息无效".into());
        }
        let client = agent();
        let mut url = url::Url::parse(&asset.url).map_err(|_| "安装包地址无效")?;
        let mut response = None;
        for _ in 0..6 {
            if url.scheme() != "https"
                || !matches!(url.host_str(), Some("github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com"))
            {
                return Err("GitHub 下载跳转到了不允许的地址，已拒绝".into());
            }
            let r = client.get(url.as_str()).header("User-Agent", "MyAIPDF").call().map_err(|_| "无法下载更新，请检查网络")?;
            if r.status().is_redirection() {
                let location = r.headers().get("Location").and_then(|h| h.to_str().ok()).ok_or("GitHub 下载跳转无效")?;
                url = url.join(location).map_err(|_| "GitHub 下载跳转无效")?;
            } else if r.status().is_success() {
                response = Some(r);
                break;
            } else {
                return Err("GitHub 未能提供更新安装包".into());
            }
        }
        let mut response = response.ok_or("GitHub 下载跳转次数过多")?;
        let path = directory.join(filename);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut output = options.open(&path).map_err(|_| "无法创建安装包；请使用空的更新目录并检查权限")?;
        let mut reader = response.body_mut().as_reader();
        let mut bytes = 0u64;
        let mut buffer = [0; 65536];
        loop {
            let n = reader.read(&mut buffer).map_err(|_| "更新下载中断，请重试")?;
            if n == 0 {
                break;
            }
            bytes = bytes.checked_add(n as u64).filter(|n| *n <= asset.size).ok_or("下载大小与发布记录不一致，未安装")?;
            output.write_all(buffer.get(..n).ok_or("更新下载数据无效")?).map_err(|_| "磁盘空间不足或写入安装包失败")?;
            progress(bytes, asset.size);
        }
        output.sync_all().map_err(|_| "无法完整保存更新安装包")?;
        let package = Package { path, version: release.version.clone(), size: asset.size, sha256: asset.sha256.clone() };
        verify_package(&package)?;
        Ok(package)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (release, directory, progress);
        Err("浏览器不能安装桌面更新".into())
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Installed {
    pub application: PathBuf,
    pub backup: Option<PathBuf>,
}

pub fn install(package: &Package, application: &Path) -> Result<Installed, String> {
    verify_package(package)?;
    #[cfg(target_os = "macos")]
    {
        macos::install(package, application)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = application;
        Err("此安装包仅适用于 Apple Silicon macOS".into())
    }
}

/// Used by the first delta's signed, user-opened installer and by headless verification.
pub fn install_delta_directory(directory: &Path, application: &Path) -> Result<Installed, String> {
    #[cfg(target_os = "macos")]
    {
        macos::install_directory(directory, application, None)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (directory, application);
        Err("增量安装仅适用于 Apple Silicon macOS".into())
    }
}
