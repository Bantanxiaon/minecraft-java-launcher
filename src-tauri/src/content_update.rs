//! 内容与整合包更新计划：updates / installs / removals / dependency changes / conflicts /
//! 用户文件保护。纯逻辑部分独立于此，便于无 Tauri 上下文的单元测试。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanAction {
    Install,
    Update,
    Remove,
    Skip,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFile {
    pub action: PlanAction,
    pub relative_path: String,
    pub file_name: String,
    pub old_sha256: Option<String>,
    pub new_sha1: Option<String>,
    pub new_sha256: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContentUpdatePlan {
    pub updates: Vec<PlannedFile>,
    pub installs: Vec<String>,
    pub removals: Vec<String>,
    pub dependency_changes: Vec<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModpackUpdatePlan {
    pub instance_id: i64,
    pub pack_version: Option<String>,
    pub files: Vec<PlannedFile>,
    pub installs: Vec<String>,
    pub updates: Vec<String>,
    pub removals: Vec<String>,
    pub dependency_changes: Vec<String>,
    pub conflicts: Vec<String>,
    pub protected_user_files: Vec<String>,
}

impl ModpackUpdatePlan {
    pub fn is_noop(&self) -> bool {
        self.files
            .iter()
            .all(|file| file.action == PlanAction::Skip)
            && self.conflicts.is_empty()
    }
}

/// 整合包文件的既有状态（用于无副作用分类）。
#[derive(Debug, Clone)]
pub struct PackFileState {
    /// 旧整合包记录的文件哈希（pack_owned_files）。
    pub pack_sha256: Option<String>,
    /// 当前磁盘哈希（无文件为 None）。
    pub disk_sha256: Option<String>,
    pub is_save: bool,
    pub is_config: bool,
    pub allow_saves_overwrite: bool,
}

pub enum PackFileClassification {
    Install,
    Update,
    Skip,
    Conflict(String),
}

/// 对单个新整合包文件做无副作用分类：
/// - 旧包拥有且磁盘未被用户改动 → 内容一致 Skip，不一致 Update；
/// - 旧包拥有但用户改动 → 存档默认保护，其余报 Conflict 保留用户版本；
/// - 旧包不拥有但磁盘已有同名文件 → 用户自加，Conflict 保护；
/// - 旧包不拥有且磁盘不存在 → Install。
pub fn classify_pack_file(new_sha256: &str, state: &PackFileState) -> PackFileClassification {
    let Some(pack_sha256) = state.pack_sha256.as_deref() else {
        if state.disk_sha256.is_some() {
            return PackFileClassification::Conflict(
                "该文件不是旧整合包管理的内容，已被用户添加或修改，保留现有版本。".into(),
            );
        }
        return PackFileClassification::Install;
    };
    let disk_sha256 = state.disk_sha256.as_deref();
    if disk_sha256.is_none() {
        return PackFileClassification::Install;
    }
    if disk_sha256 == Some(pack_sha256) {
        if pack_sha256.eq_ignore_ascii_case(new_sha256) {
            return PackFileClassification::Skip;
        }
        return PackFileClassification::Update;
    }
    if state.is_save && !state.allow_saves_overwrite {
        return PackFileClassification::Conflict(
            "新整合包包含存档目录，现有存档已保留未覆盖。".into(),
        );
    }
    if state.is_save {
        return PackFileClassification::Conflict("存档已被修改，需要你确认后才能覆盖。".into());
    }
    if state.is_config {
        return PackFileClassification::Conflict("配置文件已被用户修改，保留现有版本。".into());
    }
    PackFileClassification::Conflict(
        "该文件与现有内容不一致且不是整合包原版，已保留现有版本。".into(),
    )
}

/// 依赖差集：返回 (新增依赖, 不再需要的依赖)，忽略大小写与首尾空白。
pub fn dependency_delta(old: &[String], new: &[String]) -> (Vec<String>, Vec<String>) {
    let normalize = |value: &str| value.trim().to_ascii_lowercase();
    let old_set: std::collections::HashSet<String> =
        old.iter().map(|value| normalize(value)).collect();
    let new_set: std::collections::HashSet<String> =
        new.iter().map(|value| normalize(value)).collect();
    let added: Vec<String> = new
        .iter()
        .filter(|value| !old_set.contains(&normalize(value)))
        .cloned()
        .collect();
    let removed: Vec<String> = old
        .iter()
        .filter(|value| !new_set.contains(&normalize(value)))
        .cloned()
        .collect();
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(
        relative_path: &str,
        pack_sha256: Option<&str>,
        disk_sha256: Option<&str>,
    ) -> PackFileState {
        let relative = relative_path.replace('\\', "/").to_ascii_lowercase();
        PackFileState {
            pack_sha256: pack_sha256.map(str::to_string),
            disk_sha256: disk_sha256.map(str::to_string),
            is_save: relative.starts_with("saves/"),
            is_config: relative.starts_with("config/"),
            allow_saves_overwrite: false,
        }
    }

    #[test]
    fn dependency_delta_detects_added_and_removed() {
        let (added, removed) = dependency_delta(
            &["cloth-config".into(), " Fabric-API ".into(), "fzzy".into()],
            &["fabric-api".into(), "expandability".into()],
        );
        assert_eq!(added, vec!["expandability"]);
        assert_eq!(removed, vec!["cloth-config", "fzzy"]);
    }

    #[test]
    fn pack_classification_protects_user_content() {
        // 整合包拥有的未修改文件 → 内容变化时更新。
        assert!(matches!(
            classify_pack_file("new", &state("mods/a.jar", Some("old"), Some("old"))),
            PackFileClassification::Update
        ));
        assert!(matches!(
            classify_pack_file("same", &state("mods/a.jar", Some("same"), Some("same"))),
            PackFileClassification::Skip
        ));
        // 用户改动过的配置 → 保护。
        assert!(matches!(
            classify_pack_file("new", &state("config/x.toml", Some("old"), Some("user"))),
            PackFileClassification::Conflict(_)
        ));
        // 用户自加的同名模组（不在旧包记录中）→ 保护。
        assert!(matches!(
            classify_pack_file("new", &state("mods/b.jar", None, Some("user"))),
            PackFileClassification::Conflict(_)
        ));
        // 全新文件 → 安装。
        assert!(matches!(
            classify_pack_file("new", &state("mods/c.jar", None, None)),
            PackFileClassification::Install
        ));
        // 存档默认保护。
        assert!(matches!(
            classify_pack_file(
                "new",
                &state("saves/world/level.dat", Some("old"), Some("user"))
            ),
            PackFileClassification::Conflict(_)
        ));
    }
}
