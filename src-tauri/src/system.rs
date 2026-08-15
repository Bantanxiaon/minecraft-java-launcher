use super::{open_database, LauncherError};
use std::{path::PathBuf, process::Command};
use tauri::AppHandle;

#[tauri::command]
pub(crate) fn open_instance_directory(
    app: AppHandle,
    instance_id: i64,
    section: String,
) -> Result<String, LauncherError> {
    let connection = open_database(&app)?;
    let root: String = connection
        .query_row(
            "SELECT root_path FROM instances WHERE id=?1",
            [instance_id],
            |row| row.get(0),
        )
        .map_err(|_| LauncherError::validation("实例不存在。"))?;
    let game = PathBuf::from(root).join(".minecraft");
    let directory = match section.as_str() {
        "game" => game,
        "mods" => game.join("mods"),
        "resourcepacks" => game.join("resourcepacks"),
        "shaderpacks" => game.join("shaderpacks"),
        "saves" => game.join("saves"),
        "logs" => game.join("logs"),
        _ => return Err(LauncherError::validation("不支持的实例目录。")),
    };
    std::fs::create_dir_all(&directory)
        .map_err(|error| LauncherError::storage(format!("无法创建目录：{error}")))?;
    Command::new("explorer.exe")
        .arg(&directory)
        .spawn()
        .map_err(|error| LauncherError::storage(format!("无法打开文件夹：{error}")))?;
    Ok(directory.to_string_lossy().into_owned())
}
