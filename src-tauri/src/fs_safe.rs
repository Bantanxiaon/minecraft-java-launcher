//! Windows 路径与文件名的统一安全层。

use crate::LauncherError;
use std::path::{Path, PathBuf};

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
