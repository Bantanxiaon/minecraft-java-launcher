//! Windows 路径与文件名的统一安全层。

use crate::LauncherError;
use std::path::{Component, Path, PathBuf};

const WINDOWS_RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn validate_windows_filename(name: &str) -> Result<(), LauncherError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
        return Err(LauncherError::validation("文件名无效。"));
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Err(LauncherError::validation("文件名不能以空格或点结尾。"));
    }
    if name.chars().any(|character| {
        matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )
    }) {
        return Err(LauncherError::validation("文件名包含 Windows 禁用字符。"));
    }
    let stem = trimmed
        .split('.')
        .next()
        .unwrap_or(trimmed)
        .to_ascii_uppercase();
    if WINDOWS_RESERVED.contains(&stem.as_str()) {
        return Err(LauncherError::validation(
            "文件名使用了 Windows 保留设备名。",
        ));
    }
    Ok(())
}

pub fn ensure_canonical_child(root: &Path, target: &Path) -> Result<PathBuf, LauncherError> {
    let root = std::fs::canonicalize(root)
        .map_err(|error| LauncherError::storage(format!("无法解析根目录：{error}")))?;
    let target = std::fs::canonicalize(target)
        .map_err(|error| LauncherError::validation(format!("无法解析目标路径：{error}")))?;
    if !target.starts_with(&root) {
        return Err(LauncherError::validation("路径越出了允许的目录范围。"));
    }
    Ok(target)
}

/// Windows 上尝试 Copy-on-Write reflink（`FSCTL_DUPLICATE_EXTENTS_TO_FILE`），
/// 成功返回 true：共享底层数据块但写入互不影响，用于克隆实例的 libraries/assets 去重。
/// 不支持或失败时返回 Ok(false)，调用方回退为普通复制。
#[cfg(windows)]
pub fn reflink_copy_file(source: &Path, target: &Path) -> Result<bool, LauncherError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_SHARE_READ, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Ioctl::{
        DUPLICATE_EXTENTS_DATA, FSCTL_DUPLICATE_EXTENTS_TO_FILE,
    };
    use windows_sys::Win32::System::IO::DeviceIoControl;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let wide = |value: &Path| -> Vec<u16> {
        value
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    };
    let source_wide = wide(source);
    let target_wide = wide(target);
    unsafe {
        let source_handle = CreateFileW(
            source_wide.as_ptr(),
            FILE_GENERIC_READ,
            FILE_SHARE_READ,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if source_handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
            || source_handle.is_null()
        {
            return Ok(false);
        }
        let target_handle = CreateFileW(
            target_wide.as_ptr(),
            FILE_GENERIC_WRITE,
            FILE_SHARE_READ,
            std::ptr::null_mut(),
            windows_sys::Win32::Storage::FileSystem::CREATE_NEW,
            0,
            std::ptr::null_mut(),
        );
        if target_handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE
            || target_handle.is_null()
        {
            windows_sys::Win32::Foundation::CloseHandle(source_handle);
            return Ok(false);
        }
        let data = DUPLICATE_EXTENTS_DATA {
            FileHandle: source_handle,
            SourceFileOffset: 0,
            TargetFileOffset: 0,
            ByteCount: std::fs::metadata(source)
                .map(|metadata| metadata.len() as i64)
                .unwrap_or(0),
        };
        let mut returned: u32 = 0;
        let ok = DeviceIoControl(
            target_handle,
            FSCTL_DUPLICATE_EXTENTS_TO_FILE,
            &data as *const _ as *const _,
            std::mem::size_of::<DUPLICATE_EXTENTS_DATA>() as u32,
            std::ptr::null_mut(),
            0,
            &mut returned,
            std::ptr::null_mut(),
        );
        windows_sys::Win32::Foundation::CloseHandle(source_handle);
        windows_sys::Win32::Foundation::CloseHandle(target_handle);
        if ok == 0 {
            let _ = std::fs::remove_file(target);
            return Ok(false);
        }
    }
    Ok(true)
}

#[cfg(not(windows))]
pub fn reflink_copy_file(_source: &Path, _target: &Path) -> Result<bool, LauncherError> {
    Ok(false)
}

/// 文件移动事务：每次 move 记录反向操作，rollback 时 LIFO 回滚。
pub struct FsTransaction {
    pub id: String,
    undo: Vec<(PathBuf, PathBuf)>,
    committed: bool,
}

impl FsTransaction {
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            undo: Vec::new(),
            committed: false,
        }
    }

    pub fn move_with_undo(&mut self, from: &Path, to: &Path) -> Result<(), LauncherError> {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        std::fs::rename(from, to).map_err(|error| LauncherError::storage(error.to_string()))?;
        self.undo.push((to.to_path_buf(), from.to_path_buf()));
        Ok(())
    }

    pub fn commit(mut self) {
        log::info!("fs transaction committed: {}", self.id);
        self.committed = true;
    }

    pub fn rollback(mut self) -> Result<(), LauncherError> {
        if self.committed {
            return Ok(());
        }
        while let Some((moved_to, original)) = self.undo.pop() {
            if moved_to.exists() && !original.exists() {
                std::fs::rename(&moved_to, &original)
                    .map_err(|error| LauncherError::storage(error.to_string()))?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveLimits {
    pub max_entries: usize,
    pub max_total_uncompressed: u64,
    pub max_single_file: u64,
    pub max_compression_ratio: f64,
    pub reject_symlinks: bool,
}

impl Default for ArchiveLimits {
    fn default() -> Self {
        Self {
            max_entries: 100_000,
            max_total_uncompressed: 10 * 1024 * 1024 * 1024,
            max_single_file: 2 * 1024 * 1024 * 1024,
            max_compression_ratio: 200.0,
            reject_symlinks: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArchiveExtractReport {
    pub entries: usize,
    pub files: usize,
    pub bytes: u64,
}

/// 统一安全 ZIP 解压：防 Zip Slip、绝对路径、盘符、UNC、符号链接、Zip Bomb 与单文件超限。
pub fn extract_zip_securely(
    archive_path: &Path,
    destination: &Path,
    limits: &ArchiveLimits,
) -> Result<ArchiveExtractReport, LauncherError> {
    let file = std::fs::File::open(archive_path)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("ZIP 无效：{error}")))?;
    if archive.len() > limits.max_entries {
        return Err(LauncherError::validation("压缩包条目数超过安全限制。"));
    }
    std::fs::create_dir_all(destination)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut total_uncompressed = 0u64;
    let mut files = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        if limits.reject_symlinks
            && entry
                .unix_mode()
                .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(LauncherError::validation("压缩包包含符号链接条目。"));
        }
        let name = entry.name().to_string();
        let path = Path::new(&name);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::Prefix(_) | Component::ParentDir | Component::RootDir
                )
            })
            || name.contains("..\\")
            || name.starts_with("\\\\")
        {
            return Err(LauncherError::validation("压缩包包含非法路径条目。"));
        }
        let Some(stem) = path
            .file_name()
            .and_then(|value| value.to_str())
            .and_then(|value| value.split('.').next())
        else {
            continue;
        };
        if WINDOWS_RESERVED.contains(&stem.to_ascii_uppercase().as_str())
            || name.ends_with('.')
            || name.ends_with(' ')
        {
            return Err(LauncherError::validation("压缩包包含 Windows 非法文件名。"));
        }
        let size = entry.size();
        if size > limits.max_single_file {
            return Err(LauncherError::validation("压缩包单个文件超过安全限制。"));
        }
        total_uncompressed = total_uncompressed.saturating_add(size);
        if total_uncompressed > limits.max_total_uncompressed {
            return Err(LauncherError::validation("压缩包解压后超过安全限制。"));
        }
        if entry.compressed_size() > 0
            && (size as f64 / entry.compressed_size() as f64) > limits.max_compression_ratio
        {
            return Err(LauncherError::validation(
                "压缩包压缩比异常，可能是 Zip Bomb。",
            ));
        }
        let output = destination.join(path);
        if entry.is_dir() {
            std::fs::create_dir_all(&output)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            continue;
        }
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let mut out = std::fs::File::create(&output)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        files += 1;
    }
    Ok(ArchiveExtractReport {
        entries: archive.len(),
        files,
        bytes: total_uncompressed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, content) in entries {
            archive.start_file(*name, options).unwrap();
            archive.write_all(content).unwrap();
        }
        archive.finish().unwrap();
    }

    #[test]
    fn rejects_traversal_and_absolute_paths() {
        let dir = std::env::temp_dir().join(format!("sh-zip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("evil.zip");
        write_zip(&zip_path, &[("../evil.txt", b"x"), ("C:\\evil.txt", b"x")]);
        let result = extract_zip_securely(&zip_path, &dir.join("out"), &ArchiveLimits::default());
        assert!(result.is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn extracts_safe_archive() {
        let dir = std::env::temp_dir().join(format!("sh-zip-ok-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("ok.zip");
        write_zip(
            &zip_path,
            &[("mods/a.jar", b"jar"), ("config/x.txt", b"cfg")],
        );
        let report =
            extract_zip_securely(&zip_path, &dir.join("out"), &ArchiveLimits::default()).unwrap();
        assert_eq!(report.files, 2);
        assert!(dir.join("out").join("mods").join("a.jar").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fs_transaction_rolls_back_moves_in_reverse_order() {
        let dir = std::env::temp_dir().join(format!("sh-tx-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.txt");
        let b = dir.join("b.txt");
        std::fs::write(&a, "a").unwrap();
        let mut tx = FsTransaction::new("t1");
        tx.move_with_undo(&a, &b).unwrap();
        assert!(!a.exists() && b.exists());
        tx.rollback().unwrap();
        assert!(a.exists() && !b.exists(), "回滚应恢复原路径");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reflink_copy_is_independent_when_supported() {
        let dir = std::env::temp_dir().join(format!("sh-reflink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("source.bin");
        let target = dir.join("target.bin");
        std::fs::write(&source, vec![7u8; 4096]).unwrap();
        match reflink_copy_file(&source, &target) {
            Ok(true) => {
                assert_eq!(std::fs::read(&target).unwrap(), vec![7u8; 4096]);
                // CoW 语义：改写目标不得影响源文件（reflink 不是 hardlink）。
                std::fs::write(&target, vec![9u8; 4096]).unwrap();
                assert_eq!(std::fs::read(&source).unwrap(), vec![7u8; 4096]);
            }
            Ok(false) => {}
            Err(error) => panic!("reflink 不应报错：{}", error.message),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
