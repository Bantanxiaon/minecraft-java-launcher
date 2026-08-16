use super::*;

pub(crate) async fn run_clone_acceptance(
    app: AppHandle,
    source_name: String,
    copy_saves: bool,
) -> Result<serde_json::Value, LauncherError> {
    let instance = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.name == source_name)
        .ok_or_else(|| LauncherError::validation("验收源实例不存在，请先安装对应版本。"))?;
    let source_game = PathBuf::from(&instance.root_path).join(".minecraft");
    let cloned = clone_instance(
        app.clone(),
        instance.id,
        format!("{source_name} Clone QA"),
        copy_saves,
    )?;
    let target_game = PathBuf::from(&cloned.root_path).join(".minecraft");
    let mut missing: Vec<String> = Vec::new();
    for directory in ["mods", "config", "resourcepacks", "shaderpacks", "versions"] {
        if source_game.join(directory).is_dir() && !target_game.join(directory).is_dir() {
            missing.push(directory.to_string());
        }
    }
    if source_game.join("libraries").is_dir() && !target_game.join("libraries").is_dir() {
        missing.push("libraries".into());
    }
    if source_game.join("assets").is_dir() && !target_game.join("assets").is_dir() {
        missing.push("assets".into());
    }
    let saves_copied = target_game.join("saves").is_dir()
        && target_game
            .join("saves")
            .read_dir()
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .next()
            .is_some();
    if copy_saves && !saves_copied {
        missing.push("saves".into());
    }
    if !missing.is_empty() {
        return Err(LauncherError::storage(format!(
            "克隆验收文件结构不完整：{}",
            missing.join("、")
        )));
    }
    let report = content_reconcile::reconcile_scan(app.clone(), cloned.id)?;
    Ok(serde_json::json!({
        "status": "passed",
        "sourceInstanceId": instance.id,
        "clonedInstanceId": cloned.id,
        "copySaves": copy_saves,
        "reconcile": {
            "dbMissingOnDisk": report.db_missing_on_disk.len(),
            "diskMissingInDb": report.disk_missing_in_db.len(),
            "duplicateGroups": report.duplicate_groups.len()
        },
        "completedAt": chrono_like_timestamp()
    }))
}

pub(crate) async fn run_modpack_update_acceptance(
    app: AppHandle,
    instance_id: i64,
    source_path: String,
) -> Result<serde_json::Value, LauncherError> {
    let plan = update_modrinth_modpack(app.clone(), instance_id, source_path).await?;
    Ok(serde_json::json!({
        "status": "passed",
        "instanceId": instance_id,
        "packVersion": plan.pack_version,
        "installs": plan.installs.len(),
        "updates": plan.updates.len(),
        "removals": plan.removals.len(),
        "conflicts": plan.conflicts.len(),
        "protectedUserFiles": plan.protected_user_files.len(),
        "completedAt": chrono_like_timestamp()
    }))
}

pub(crate) async fn run_vanilla_install_acceptance(
    app: AppHandle,
    game_version: String,
) -> Result<serde_json::Value, LauncherError> {
    validate_loader_token(&game_version)?;
    let instance_name = format!("Acceptance Vanilla {game_version}");
    let instance = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.name == instance_name)
        .map(Ok)
        .unwrap_or_else(|| {
            create_vanilla_instance(app.clone(), instance_name, game_version.clone())
        })?;
    let manifest = fetch_version_manifest(false).await?;
    let version = manifest
        .versions
        .into_iter()
        .find(|version| version.id == game_version)
        .ok_or_else(|| LauncherError::validation("验收版本不在 Mojang release 清单中。"))?;
    let preview = install_vanilla_client(app, instance.id, version.url, version.sha1).await?;
    let game = PathBuf::from(&instance.root_path).join(".minecraft");
    let client = game
        .join("versions")
        .join(&game_version)
        .join(format!("{game_version}.jar"));
    if !client.is_file()
        || !game.join("libraries").is_dir()
        || !game.join("assets").join("objects").is_dir()
    {
        return Err(LauncherError::storage(
            "安装命令已返回，但验收文件结构不完整。",
        ));
    }
    Ok(serde_json::json!({
        "status":"passed",
        "gameVersion":game_version,
        "instanceId":instance.id,
        "instanceRoot":instance.root_path,
        "clientBytes":preview.client_bytes,
        "libraryCount":preview.library_count,
        "libraryBytes":preview.library_bytes,
        "completedAt":chrono_like_timestamp()
    }))
}

pub(crate) async fn run_vanilla_launch_acceptance(
    app: AppHandle,
    game_version: String,
    java_path: String,
    loader_type: String,
) -> Result<serde_json::Value, LauncherError> {
    validate_loader_token(&game_version)?;
    validate_loader_type(&loader_type)?;
    let instance_name = if loader_type == "vanilla" {
        format!("Acceptance Vanilla {game_version}")
    } else {
        format!("Acceptance {loader_type} {game_version}")
    };
    let instance = list_instances(app)?
        .into_iter()
        .find(|instance| instance.name == instance_name && instance.status == "ready")
        .ok_or_else(|| LauncherError::validation("请先完成对应版本的全量安装验收。"))?;
    let java = PathBuf::from(java_path);
    let runtime = inspect_java_runtime(&java)?;
    if !runtime.is_64_bit {
        return Err(LauncherError::validation("验收 Java 不是 64 位。"));
    }
    let game = PathBuf::from(&instance.root_path).join(".minecraft");
    let version_directory = game.join("versions").join(&game_version);
    let metadata_path = if loader_type == "vanilla" {
        version_directory.join(format!("{game_version}.json"))
    } else {
        version_directory.join("launcher-effective.json")
    };
    let metadata = fs::read(&metadata_path)
        .map_err(|error| LauncherError::storage(format!("读取验收元数据失败：{error}")))?;
    let details: serde_json::Value = serde_json::from_slice(&metadata)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let required_java = details
        .pointer("/javaVersion/majorVersion")
        .and_then(|value| value.as_u64())
        .map(|value| value as u32);
    if required_java.is_some() && runtime.major_version != required_java {
        return Err(LauncherError::validation(format!(
            "验收版本要求 Java {:?}，实际为 {:?}。",
            required_java, runtime.major_version
        )));
    }
    let arguments = build_vanilla_launch_arguments(
        &details,
        &game,
        &game_version,
        "QA_Player",
        "00000000000000000000000000000000",
        "0",
        "legacy",
        "",
        4096,
    )?;
    let log_path = game
        .join("logs")
        .join(format!("launch-acceptance-{}.log", unique_timestamp()));
    if let Some(parent) = log_path.parent() {
        fs::create_dir_all(parent).map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    let java_for_task = java.clone();
    let game_for_task = game.clone();
    let log_for_task = log_path.clone();
    let arguments_for_task = arguments.clone();
    let (process_id, observed_seconds, terminated_by_acceptance) =
        tokio::task::spawn_blocking(move || -> Result<(u32, u64, bool), LauncherError> {
            let stdout = fs::File::create(&log_for_task)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let stderr = stdout
                .try_clone()
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            let mut child = Command::new(java_for_task)
                .args(arguments_for_task)
                .current_dir(game_for_task)
                .stdin(Stdio::null())
                .stdout(Stdio::from(stdout))
                .stderr(Stdio::from(stderr))
                .spawn()
                .map_err(|error| LauncherError::storage(format!("启动验收客户端失败：{error}")))?;
            let process_id = child.id();
            let started = std::time::Instant::now();
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                if let Some(status) = child
                    .try_wait()
                    .map_err(|error| LauncherError::storage(error.to_string()))?
                {
                    let observed = started.elapsed().as_secs();
                    if status.success() && observed >= 10 {
                        let log = fs::read_to_string(&log_for_task).unwrap_or_default();
                        if log.contains("Setting user:")
                            && log.contains("Sound engine started")
                            && log.contains("Created:")
                        {
                            return Ok((process_id, observed, false));
                        }
                    }
                    return Err(LauncherError::storage(format!(
                        "Minecraft 在完成渲染初始化前退出：{:?}（运行 {observed} 秒）",
                        status.code(),
                    )));
                }
                if std::time::Instant::now() >= deadline {
                    child.kill().map_err(|error| {
                        LauncherError::storage(format!("停止验收客户端失败：{error}"))
                    })?;
                    let _ = child.wait();
                    return Ok((process_id, 30, true));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .await
        .map_err(|error| LauncherError::storage(format!("启动验收任务异常：{error}")))??;
    Ok(serde_json::json!({
        "status":"passed",
        "gameVersion":game_version,
        "loaderType":loader_type,
        "javaVersion":runtime.version,
        "processId":process_id,
        "observedSeconds":observed_seconds,
        "terminatedByAcceptance":terminated_by_acceptance,
        "logPath":log_path.to_string_lossy(),
        "completedAt":chrono_like_timestamp()
    }))
}

pub(crate) async fn run_loader_install_acceptance(
    app: AppHandle,
    game_version: String,
    loader_type: String,
    java_path: String,
) -> Result<serde_json::Value, LauncherError> {
    validate_loader_type(&loader_type)?;
    if loader_type == "vanilla" {
        return Err(LauncherError::validation("加载器验收不能选择 Vanilla。"));
    }
    let base_name = format!("Acceptance Vanilla {game_version}");
    let base = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.name == base_name && instance.status == "ready")
        .ok_or_else(|| LauncherError::validation("请先完成 Vanilla 全量安装验收。"))?;
    let instance_name = format!("Acceptance {loader_type} {game_version}");
    let instance = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.name == instance_name)
        .map(Ok)
        .unwrap_or_else(|| {
            create_instance_profile(
                app.clone(),
                instance_name,
                game_version.clone(),
                loader_type.clone(),
            )
        })?;
    let connection = open_database(&app)?;
    let game_verified: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM installation_states WHERE instance_id=?1 AND component_kind='game' AND status='verified'",
            [instance.id],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    if game_verified == 0 {
        let source = PathBuf::from(&base.root_path).join(".minecraft");
        let destination = PathBuf::from(&instance.root_path).join(".minecraft");
        tokio::task::spawn_blocking(move || copy_world_directory(&source, &destination))
            .await
            .map_err(|error| LauncherError::storage(error.to_string()))??;
        connection.execute(
            "INSERT INTO installation_states(instance_id,component_kind,component_key,status) VALUES(?1,'game',?2,'verified') ON CONFLICT(instance_id,component_kind,component_key) DO UPDATE SET status='verified'",
            params![instance.id, game_version],
        ).map_err(|error| LauncherError::storage(error.to_string()))?;
        connection
            .execute(
                "UPDATE instances SET status='loader_missing' WHERE id=?1",
                [instance.id],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    }
    drop(connection);
    let versions = list_loader_versions(loader_type.clone(), game_version.clone()).await?;
    let loader_version = versions
        .first()
        .cloned()
        .ok_or_else(|| LauncherError::validation("没有可验收的加载器版本。"))?;
    let installed = if matches!(loader_type.as_str(), "fabric" | "quilt") {
        install_profile_loader(app, instance.id, loader_version.clone()).await?
    } else {
        install_java_loader(app, instance.id, loader_version.clone(), java_path).await?
    };
    let effective = PathBuf::from(&installed.root_path)
        .join(".minecraft")
        .join("versions")
        .join(&game_version)
        .join("launcher-effective.json");
    if installed.status != "ready" || !effective.is_file() {
        return Err(LauncherError::storage(
            "加载器安装返回后未生成就绪的有效启动 profile。",
        ));
    }
    Ok(serde_json::json!({
        "status":"passed",
        "gameVersion":game_version,
        "loaderType":loader_type,
        "loaderVersion":loader_version,
        "instanceId":installed.id,
        "instanceRoot":installed.root_path,
        "effectiveProfile":effective.to_string_lossy(),
        "completedAt":chrono_like_timestamp()
    }))
}
