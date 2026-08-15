use super::{open_database, LauncherError};
use serde::Serialize;
use std::{
    fs, io,
    path::{Component, Path, PathBuf},
};
use tauri::AppHandle;
use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportResult {
    path: String,
    files: u64,
    bytes: u64,
}

fn safe_archive_name(relative: &Path) -> Result<String, LauncherError> {
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(value) => parts.push(value.to_string_lossy().into_owned()),
            _ => return Err(LauncherError::validation("导出内容包含异常路径。")),
        }
    }
    if parts.is_empty() {
        return Err(LauncherError::validation("导出内容路径为空。"));
    }
    Ok(parts.join("/"))
}

fn append_tree(
    writer: &mut ZipWriter<fs::File>,
    source_root: &Path,
    archive_root: &str,
    files: &mut u64,
    bytes: &mut u64,
) -> Result<(), LauncherError> {
    if !source_root.exists() {
        return Ok(());
    }
    let mut pending = vec![source_root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| LauncherError::storage(format!("读取导出文件失败：{error}")))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(&path)
                .map_err(|error| LauncherError::storage(format!("读取导出目录失败：{error}")))?
            {
                pending.push(
                    entry
                        .map_err(|error| LauncherError::storage(error.to_string()))?
                        .path(),
                );
            }
            continue;
        }
        let relative = path
            .strip_prefix(source_root)
            .map_err(|_| LauncherError::validation("导出文件不在实例目录中。"))?;
        let relative_name = safe_archive_name(relative)?;
        let archive_name = if relative_name.is_empty() {
            archive_root.to_string()
        } else {
            format!("{archive_root}/{relative_name}")
        };
        writer
            .start_file(
                archive_name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|error| LauncherError::storage(format!("写入整合包失败：{error}")))?;
        let mut input = fs::File::open(&path)
            .map_err(|error| LauncherError::storage(format!("读取导出文件失败：{error}")))?;
        let copied = io::copy(&mut input, writer)
            .map_err(|error| LauncherError::storage(format!("压缩导出文件失败：{error}")))?;
        *files += 1;
        *bytes += copied;
    }
    Ok(())
}

fn append_file(
    writer: &mut ZipWriter<fs::File>,
    source: &Path,
    archive_name: &str,
    files: &mut u64,
    bytes: &mut u64,
) -> Result<(), LauncherError> {
    let metadata = fs::symlink_metadata(source)
        .map_err(|error| LauncherError::storage(format!("读取导出文件失败：{error}")))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Ok(());
    }
    writer
        .start_file(
            archive_name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| LauncherError::storage(format!("写入整合包失败：{error}")))?;
    let mut input = fs::File::open(source)
        .map_err(|error| LauncherError::storage(format!("读取导出文件失败：{error}")))?;
    let copied = io::copy(&mut input, writer)
        .map_err(|error| LauncherError::storage(format!("压缩导出文件失败：{error}")))?;
    *files += 1;
    *bytes += copied;
    Ok(())
}

#[tauri::command]
pub(crate) fn export_instance_modpack(
    app: AppHandle,
    instance_id: i64,
    destination: String,
    include_saves: bool,
) -> Result<ExportResult, LauncherError> {
    let destination = PathBuf::from(destination);
    if !destination.is_absolute()
        || destination
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
    {
        return Err(LauncherError::validation("请选择完整的 .zip 导出位置。"));
    }
    let connection = open_database(&app)?;
    let (name, root_path, game_version, loader_type): (String, String, String, String) = connection
        .query_row(
            "SELECT name,root_path,game_version,loader_type FROM instances WHERE id=?1",
            [instance_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .map_err(|_| LauncherError::validation("要导出的实例不存在。"))?;
    let game = PathBuf::from(root_path).join(".minecraft");
    if !game.is_dir() {
        return Err(LauncherError::validation("实例游戏目录不存在。"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let temporary = destination.with_extension("zip.part");
    if temporary.exists() {
        fs::remove_file(&temporary).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let output = fs::File::create(&temporary)
        .map_err(|error| LauncherError::storage(format!("无法创建导出文件：{error}")))?;
    let mut writer = ZipWriter::new(output);
    let manifest = serde_json::json!({
        "formatVersion": 1,
        "name": name,
        "minecraftVersion": game_version,
        "loader": loader_type,
        "createdBy": "SH启动器",
        "includesSaves": include_saves,
        "privacy": "不包含账户、Microsoft Token、启动器数据库或凭据"
    });
    writer
        .start_file(
            "sh-modpack.json",
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    serde_json::to_writer_pretty(&mut writer, &manifest)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut files = 1;
    let mut bytes = 0;
    for directory in ["mods", "config", "resourcepacks", "shaderpacks"] {
        append_tree(
            &mut writer,
            &game.join(directory),
            directory,
            &mut files,
            &mut bytes,
        )?;
    }
    for file_name in ["options.txt", "servers.dat"] {
        let path = game.join(file_name);
        if path.is_file() {
            append_file(&mut writer, &path, file_name, &mut files, &mut bytes)?;
        }
    }
    if include_saves {
        append_tree(
            &mut writer,
            &game.join("saves"),
            "saves",
            &mut files,
            &mut bytes,
        )?;
    }
    writer
        .finish()
        .map_err(|error| LauncherError::storage(format!("完成整合包压缩失败：{error}")))?;
    if destination.exists() {
        fs::remove_file(&destination).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    fs::rename(&temporary, &destination)
        .map_err(|error| LauncherError::storage(format!("保存整合包失败：{error}")))?;
    Ok(ExportResult {
        path: destination.to_string_lossy().into_owned(),
        files,
        bytes,
    })
}

#[tauri::command]
pub(crate) fn export_world(
    app: AppHandle,
    content_id: i64,
    destination: String,
) -> Result<ExportResult, LauncherError> {
    let destination = PathBuf::from(destination);
    if !destination.is_absolute()
        || destination
            .extension()
            .and_then(|value| value.to_str())
            .is_none_or(|value| !value.eq_ignore_ascii_case("zip"))
    {
        return Err(LauncherError::validation("请选择完整的 .zip 导出位置。"));
    }
    let connection = open_database(&app)?;
    let (file_name, root_path): (String, String) = connection
        .query_row(
            "SELECT c.file_name,i.root_path FROM content_items c JOIN instances i ON i.id=c.instance_id WHERE c.id=?1 AND c.kind='world'",
            [content_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| LauncherError::validation("要导出的存档不存在。"))?;
    if Path::new(&file_name).components().count() != 1 {
        return Err(LauncherError::validation("存档名称包含异常路径。"));
    }
    let source = PathBuf::from(root_path)
        .join(".minecraft")
        .join("saves")
        .join(&file_name);
    if !source.join("level.dat").is_file() {
        return Err(LauncherError::validation("存档不完整，缺少 level.dat。"));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let temporary = destination.with_extension("zip.part");
    let output = fs::File::create(&temporary)
        .map_err(|error| LauncherError::storage(format!("无法创建存档压缩包：{error}")))?;
    let mut writer = ZipWriter::new(output);
    let mut files = 0;
    let mut bytes = 0;
    append_tree(&mut writer, &source, &file_name, &mut files, &mut bytes)?;
    writer
        .finish()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if destination.exists() {
        fs::remove_file(&destination).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    fs::rename(&temporary, &destination)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(ExportResult {
        path: destination.to_string_lossy().into_owned(),
        files,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_archive_names_are_normalized() {
        assert_eq!(
            safe_archive_name(Path::new("mods").join("example.jar").as_path()).unwrap(),
            "mods/example.jar"
        );
    }

    #[test]
    fn export_archive_names_reject_parent_paths() {
        assert!(safe_archive_name(Path::new("..\\account.json")).is_err());
        assert!(safe_archive_name(Path::new("C:\\token.txt")).is_err());
    }
}
