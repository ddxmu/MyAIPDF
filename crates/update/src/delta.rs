//! Version-bound binary deltas. No installer scripts, links, or arbitrary output paths.
use crate::{MAX_PACKAGE_BYTES, parse_version, valid_digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path};

pub const INSTALLER: &str = "MyAIPDF增量安装.app";
pub const MAX_FILE_BYTES: u64 = 512 * 1024 * 1024;
const MAX_BUNDLE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_FILES: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Fingerprint {
    pub sha256: String,
    pub size: u64,
    pub executable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Copy,
    Bsdiff,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Payload {
    pub file: String,
    pub kind: Kind,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub path: String,
    pub before: Option<Fingerprint>,
    pub after: Option<Fingerprint>,
    pub payload: Option<Payload>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub from_version: String,
    pub version: String,
    pub architecture: String,
    pub files: Vec<File>,
}

pub fn valid_path(path: &str) -> bool {
    path.starts_with("Contents/")
        && path.len() <= 1024
        && !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
        && Path::new(path).components().all(|p| matches!(p, Component::Normal(_)))
}

pub fn fingerprint(path: &Path) -> Result<Fingerprint, String> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| "无法读取增量文件信息")?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return Err("增量文件不是普通文件或超过大小限制".into());
    }
    let mut input = std::fs::File::open(path).map_err(|_| "无法读取增量文件")?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut size = 0u64;
    loop {
        let n = input.read(&mut buffer).map_err(|_| "增量文件读取失败")?;
        if n == 0 {
            break;
        }
        size = size.checked_add(n as u64).filter(|s| *s <= MAX_FILE_BYTES).ok_or("增量文件过大")?;
        hash.update(buffer.get(..n).ok_or("增量文件读取无效")?);
    }
    if size != meta.len() {
        return Err("文件在读取期间发生变化，已停止更新".into());
    }
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let executable = false;
    Ok(Fingerprint { sha256: hash.finalize().iter().map(|b| format!("{b:02x}")).collect(), size, executable })
}

/// Complete file inventory: an altered base must never be used to reconstruct a release.
pub fn snapshot(root: &Path) -> Result<BTreeMap<String, Fingerprint>, String> {
    if !std::fs::symlink_metadata(root).is_ok_and(|m| m.is_dir()) {
        return Err("应用目录不存在或是链接".into());
    }
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    let mut files = BTreeMap::new();
    let (mut bytes, mut entries) = (0u64, 0usize);
    while let Some((directory, depth)) = stack.pop() {
        if depth > 32 {
            return Err("应用目录嵌套过深".into());
        }
        for entry in std::fs::read_dir(directory).map_err(|_| "无法读取应用目录")? {
            let entry = entry.map_err(|_| "无法读取应用条目")?;
            entries = entries.checked_add(1).filter(|n| *n <= MAX_FILES * 2).ok_or("应用条目过多")?;
            let path = entry.path();
            let meta = std::fs::symlink_metadata(&path).map_err(|_| "应用条目无效")?;
            if meta.is_dir() {
                stack.push((path, depth + 1));
            } else {
                let name = path.strip_prefix(root).ok().and_then(|p| p.to_str()).ok_or("应用路径无效")?;
                if !valid_path(name) || !meta.is_file() {
                    return Err("应用包含链接、特殊文件或不支持的路径".into());
                }
                let info = fingerprint(&path)?;
                bytes = bytes.checked_add(info.size).filter(|n| *n <= MAX_BUNDLE_BYTES).ok_or("应用超过大小限制")?;
                files.insert(name.to_owned(), info);
                if files.len() > MAX_FILES {
                    return Err("应用文件过多".into());
                }
            }
        }
    }
    Ok(files)
}

impl Manifest {
    pub fn read(directory: &Path) -> Result<Self, String> {
        let path = directory.join("manifest.json");
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= 2 * 1024 * 1024) {
            return Err("增量清单无效或过大".into());
        }
        let value: Self = serde_json::from_slice(&std::fs::read(path).map_err(|_| "无法读取增量清单")?).map_err(|_| "增量清单格式无效")?;
        value.validate()?;
        Ok(value)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format != 1
            || self.architecture != "aarch64"
            || parse_version(&self.from_version).is_none()
            || !crate::is_newer(&self.version, &self.from_version)
            || self.files.is_empty()
            || self.files.len() > MAX_FILES
        {
            return Err("增量包版本、平台或格式不受支持".into());
        }
        let (mut paths, mut payloads) = (BTreeSet::new(), BTreeSet::new());
        let (mut before_bytes, mut after_bytes, mut payload_bytes) = (0u64, 0u64, 0u64);
        for file in &self.files {
            if !valid_path(&file.path) || !paths.insert(&file.path) || (file.before.is_none() && file.after.is_none()) {
                return Err("增量包包含重复或危险路径".into());
            }
            for (info, total) in [(&file.before, &mut before_bytes), (&file.after, &mut after_bytes)] {
                if let Some(info) = info {
                    if !valid_digest(&info.sha256) || info.size > MAX_FILE_BYTES {
                        return Err("增量文件校验信息无效".into());
                    }
                    *total = total.checked_add(info.size).filter(|n| *n <= MAX_BUNDLE_BYTES).ok_or("增量应用超过大小限制")?;
                }
            }
            match &file.payload {
                Some(p) => {
                    payload_bytes = payload_bytes.checked_add(p.size).filter(|n| *n <= MAX_PACKAGE_BYTES).ok_or("增量数据总量超过限制")?;
                    if p.file.is_empty()
                        || p.file.len() > 80
                        || !p.file.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'.'))
                        || p.file == "."
                        || p.file == ".."
                        || !payloads.insert(&p.file)
                        || !valid_digest(&p.sha256)
                        || p.size > MAX_PACKAGE_BYTES
                        || file.after.is_none()
                        || (matches!(p.kind, Kind::Bsdiff) && file.before.is_none())
                    {
                        return Err("增量数据文件无效".into());
                    }
                }
                None if file.after.is_some() && file.before != file.after => return Err("增量包缺少修改数据".into()),
                None => {}
            }
        }
        for path in paths {
            let mut parent = Path::new(path).parent();
            while let Some(p) = parent {
                if p.to_str().is_some_and(|p| self.files.iter().any(|f| f.path == p)) {
                    return Err("增量包的文件与目录路径冲突".into());
                }
                parent = p.parent();
            }
        }
        Ok(())
    }

    pub fn expected(&self, after: bool) -> BTreeMap<String, Fingerprint> {
        self.files.iter().filter_map(|f| if after { &f.after } else { &f.before }.as_ref().map(|info| (f.path.clone(), info.clone()))).collect()
    }

    pub fn verify_payloads(&self, directory: &Path) -> Result<(), String> {
        let expected: BTreeSet<_> = self
            .files
            .iter()
            .filter_map(|f| f.payload.as_ref().map(|p| p.file.clone()))
            .chain(std::iter::once("manifest.json".to_string()))
            .collect();
        let mut actual = BTreeSet::new();
        for entry in std::fs::read_dir(directory).map_err(|_| "无法读取增量目录")? {
            let entry = entry.map_err(|_| "无法读取增量条目")?;
            if !entry.file_type().is_ok_and(|t| t.is_file()) {
                return Err("增量目录包含链接或特殊条目".into());
            }
            actual.insert(entry.file_name().to_str().ok_or("增量文件名无效")?.to_owned());
            if actual.len() > MAX_FILES + 1 {
                return Err("增量条目过多".into());
            }
        }
        if actual != expected {
            return Err("增量包数据清单不完整或包含未声明文件".into());
        }
        for file in &self.files {
            if let Some(p) = &file.payload {
                let actual = fingerprint(&directory.join(&p.file))?;
                if actual.sha256 != p.sha256 || actual.size != p.size {
                    return Err("增量数据 SHA-256 不一致，未更新".into());
                }
            }
        }
        Ok(())
    }
}
