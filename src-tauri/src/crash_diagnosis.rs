//! Minecraft 崩溃诊断：从 crash-report / latest.log / debug.log 解析
//! MixinTransformerError 的完整 causal chain，定位 owning Mod JAR 候选。

use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
// 分类学是完整产品能力的一部分；Launcher 侧分类（LAUNCHER_*）由
// ProcessSupervisor/CrashMarker 路径使用，后续接入，先保留枚举全集。
#[allow(dead_code)]
pub(crate) enum CrashClassification {
    LauncherProcessCrash,
    LauncherWindowInvisible,
    LauncherUnhandledFrontendError,
    LauncherRustPanic,
    LauncherStartupAbort,
    GameProcessCrash,
    GameEarlyExit,
    GameJvmCrash,
    GameModLoaderCrash,
    GameMixinCrash,
    GameConnectionDisconnect,
    GameNormalExit,
    UnknownProcessFailure,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct SuspectedMod {
    pub(crate) mod_id: String,
    pub(crate) version: String,
    pub(crate) jar: String,
    pub(crate) confidence: f64,
    pub(crate) evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CrashAnalysis {
    pub(crate) classification: CrashClassification,
    pub(crate) root_exception: String,
    pub(crate) wrapper_exception: String,
    pub(crate) suspected_mods: Vec<SuspectedMod>,
    pub(crate) mixin_config: Option<String>,
    pub(crate) mixin_class: Option<String>,
    pub(crate) target_class: Option<String>,
    pub(crate) game_version: Option<String>,
    pub(crate) loader: Option<String>,
    pub(crate) loader_version: Option<String>,
    pub(crate) java_version: Option<String>,
    pub(crate) repair_actions: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ModJarInfo {
    pub file_name: String,
    pub mod_id: String,
    pub version: String,
    pub mixin_configs: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CrashInputs {
    pub crash_report: String,
    pub latest_log: String,
    pub debug_log: String,
    pub mods: Vec<ModJarInfo>,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub java_version: Option<String>,
}

const MIXIN_WRAPPER: &str = "MixinTransformerError";
const MOD_LOADER_SIGNATURES: [&str; 4] = [
    "ModLoadingException",
    "LoadingFailedException",
    "Mod Loading has failed",
    "Mod loading error has occurred",
];

fn combined_text(inputs: &CrashInputs) -> String {
    format!(
        "{}\n{}\n{}",
        inputs.crash_report, inputs.latest_log, inputs.debug_log
    )
}

fn extract_caused_by_chain(text: &str) -> Vec<String> {
    let mut chain = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Caused by:") {
            chain.push(rest.trim().to_string());
        }
    }
    chain
}

fn extract_after(needle: &str, text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.find(needle)
            .map(|index| line[index + needle.len()..].trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn extract_mixin_config(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let bytes = line.as_bytes();
        let mut index = 0usize;
        while index + 1 < bytes.len() {
            if bytes[index..].starts_with(b".mixins.json") {
                let mut start = index;
                while start > 0
                    && (bytes[start - 1].is_ascii_alphanumeric()
                        || bytes[start - 1] == b'_'
                        || bytes[start - 1] == b'-'
                        || bytes[start - 1] == b'.')
                {
                    start -= 1;
                }
                return Some(line[start..index + b".mixins.json".len()].to_string());
            }
            index += 1;
        }
        None
    })
}

fn extract_target_class(text: &str) -> Option<String> {
    extract_after("target class", text)
        .or_else(|| extract_after("@Mixin target", text))
        .or_else(|| extract_after("target ", text))
}

fn extract_mixin_class(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if !lower.contains("mixin") {
            return None;
        }
        if line.contains("mixins.json:") {
            let value = line.split("mixins.json:").nth(1).unwrap_or_default().trim();
            return if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        if line.contains(".mixins.json") {
            let value = line.split(".mixins.json").nth(1).unwrap_or_default().trim();
            return if value.is_empty() {
                None
            } else {
                Some(value.to_string())
            };
        }
        None
    })
}

fn jar_candidates_in_stack(text: &str, mods: &[ModJarInfo]) -> Vec<SuspectedMod> {
    let lower = text.to_ascii_lowercase();
    let mut result = Vec::new();
    for info in mods {
        let mut evidence = Vec::new();
        let needle = info.file_name.to_ascii_lowercase();
        if lower.contains(&needle) {
            evidence.push(format!("JAR 文件名出现在崩溃栈：{}", info.file_name));
        }
        for config in &info.mixin_configs {
            if lower.contains(&config.to_ascii_lowercase()) {
                evidence.push(format!("mixin config {} 属于该 JAR", config));
            }
        }
        if !evidence.is_empty() {
            let confidence = if evidence.len() >= 2 { 0.9 } else { 0.7 };
            result.push(SuspectedMod {
                mod_id: info.mod_id.clone(),
                version: info.version.clone(),
                jar: info.file_name.clone(),
                confidence,
                evidence,
            });
        }
    }
    result
}

pub(crate) fn analyze_crash(inputs: &CrashInputs) -> CrashAnalysis {
    let text = combined_text(inputs);
    let lower = text.to_ascii_lowercase();

    let classification = if lower.contains(&MIXIN_WRAPPER.to_ascii_lowercase())
        || lower.contains("injectionerror")
        || lower.contains("invalidmixin")
        || lower.contains("mixinapply")
    {
        CrashClassification::GameMixinCrash
    } else if MOD_LOADER_SIGNATURES
        .iter()
        .any(|signature| lower.contains(&signature.to_ascii_lowercase()))
    {
        CrashClassification::GameModLoaderCrash
    } else if lower.contains("hs_err_pid")
        || lower.contains("exception_access_violation")
        || lower.contains("outofmemoryerror")
        || lower.contains("stackoverflowerror")
    {
        CrashClassification::GameJvmCrash
    } else if lower.contains("disconnect.")
        || lower.contains("failed to log in")
        || lower.contains("connection refused")
        || lower.contains("unknown host")
    {
        CrashClassification::GameConnectionDisconnect
    } else if lower.contains("exception in server tick loop")
        || lower.contains("game crashed")
        || lower.contains("---- minecraft crash report ----")
    {
        CrashClassification::GameProcessCrash
    } else {
        CrashClassification::UnknownProcessFailure
    };

    let chain = extract_caused_by_chain(&text);
    let root_exception = chain
        .last()
        .cloned()
        .or_else(|| {
            text.lines().find_map(|line| {
                let line = line.trim();
                if line.starts_with("java.lang.") || line.starts_with("net.minecraft.") {
                    Some(line.to_string())
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| "未找到根因异常".to_string());

    let wrapper_exception = if lower.contains(&MIXIN_WRAPPER.to_ascii_lowercase()) {
        MIXIN_WRAPPER.to_string()
    } else {
        chain.first().cloned().unwrap_or_default()
    };

    let mixin_config = extract_mixin_config(&text);
    let mixin_class = extract_mixin_class(&text);
    let target_class = extract_target_class(&text);

    let mut suspected = jar_candidates_in_stack(&text, &inputs.mods);
    if let Some(config) = &mixin_config {
        for info in &inputs.mods {
            if info
                .mixin_configs
                .iter()
                .any(|candidate| candidate == config)
                && !suspected.iter().any(|s| s.jar == info.file_name)
            {
                suspected.push(SuspectedMod {
                    mod_id: info.mod_id.clone(),
                    version: info.version.clone(),
                    jar: info.file_name.clone(),
                    confidence: 0.5,
                    evidence: vec![format!("mixin config {} 属于该 JAR", config)],
                });
            }
        }
    }
    suspected.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut repair_actions = Vec::new();
    if !suspected.is_empty() {
        repair_actions.push("检查高置信度候选 Mod 的版本与前置依赖".to_string());
        repair_actions.push("临时停用候选 Mod 后重试（可回滚）".to_string());
    } else {
        repair_actions.push("当前日志不足以唯一定位冲突 Mod，建议查看详细日志".to_string());
    }
    if mixin_config.is_some() {
        repair_actions
            .push("检查该 mixin config 对应 Mod 与当前 Minecraft/加载器版本的兼容性".to_string());
    }
    repair_actions.push("收集 crash-report / latest.log / debug.log 后重试或查看诊断".to_string());

    CrashAnalysis {
        classification,
        root_exception,
        wrapper_exception,
        suspected_mods: suspected,
        mixin_config,
        mixin_class,
        target_class,
        game_version: inputs.game_version.clone(),
        loader: inputs.loader.clone(),
        loader_version: inputs.loader_version.clone(),
        java_version: inputs.java_version.clone(),
        repair_actions,
    }
}

/// 扫描 mods 目录，构建 ModJarInfo 索引（mod id / version / mixins.json 列表）。
pub(crate) fn index_mods_directory(mods_dir: &Path) -> Vec<ModJarInfo> {
    let mut result = Vec::new();
    let Ok(entries) = std::fs::read_dir(mods_dir) else {
        return result;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("jar") {
            continue;
        }
        result.push(index_jar(&path));
    }
    result
}

fn index_jar(path: &Path) -> ModJarInfo {
    let file_name = path
        .file_name()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut info = ModJarInfo {
        file_name,
        ..Default::default()
    };
    let Ok(file) = std::fs::File::open(path) else {
        return info;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return info;
    };
    for index in 0..archive.len() {
        let Ok(mut entry) = archive.by_index(index) else {
            continue;
        };
        let name = entry.name().to_string();
        if name == "META-INF/mods.toml" || name == "fabric.mod.json" || name == "quilt.mod.json" {
            let mut buffer = Vec::new();
            if std::io::Read::read_to_end(&mut entry, &mut buffer).is_err() {
                continue;
            }
            let content = String::from_utf8_lossy(&buffer).to_string();
            if name.ends_with("mods.toml") {
                if let Some(id) = extract_toml_value(&content, "modId") {
                    info.mod_id = id;
                }
                if let Some(version) = extract_toml_value(&content, "version") {
                    info.version = version;
                }
            } else {
                if let Some(id) = extract_json_value(&content, "id") {
                    info.mod_id = id;
                }
                if let Some(version) = extract_json_value(&content, "version") {
                    info.version = version;
                }
            }
        } else if name.ends_with(".mixins.json") {
            info.mixin_configs.push(name);
        }
    }
    info.mixin_configs.sort();
    info.mixin_configs.dedup();
    info
}

fn extract_toml_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        let prefix = format!("{key}=");
        trimmed
            .strip_prefix(&prefix)
            .map(|value| value.trim_matches('"').trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn extract_json_value(content: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    content.find(&needle).and_then(|index| {
        let rest = &content[index + needle.len()..];
        let start = rest.find(':')? + 1;
        let value = rest[start..].trim_start();
        let end = value.find([',', '}', '\n']).unwrap_or(value.len());
        Some(value[..end].trim().trim_matches('"').to_string())
    })
}

#[tauri::command]
pub(crate) fn analyze_crash_texts(
    crash_report: String,
    latest_log: String,
    debug_log: String,
    mods_dir: Option<String>,
    game_version: Option<String>,
    loader: Option<String>,
    loader_version: Option<String>,
    java_version: Option<String>,
) -> CrashAnalysis {
    let mods = mods_dir
        .as_deref()
        .map(Path::new)
        .map(index_mods_directory)
        .unwrap_or_default();
    analyze_crash(&CrashInputs {
        crash_report,
        latest_log,
        debug_log,
        mods,
        game_version,
        loader,
        loader_version,
        java_version,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture_mixin_crash() -> String {
        format!(
            "{}\n{}\n{}",
            "---- Minecraft Crash Report ----",
            "org.spongepowered.asm.mixin.transformer.throwables.MixinTransformerError: An unexpected critical error was encountered",
            "  at org.spongepowered.asm.mixin.transformer.MixinTransformer.transformClass(MixinTransformer.java:207) ~[mixin-0.8.5.jar!/:0.8.5]",
            // 上面格式换行拼接
        ) + "Caused by: org.spongepowered.asm.mixin.transformer.throwables.InvalidMixinException: Inconsistent @Mixin target for 'some_mod.mixins.json:com.example.SomeMixin': target class net.minecraft.world.entity.LivingEntity was not found\n  at com.example.somemod.SomeMod$Client.onClientSetup(somemod-1.2.3.jar!/com/example/somemod/SomeMod.class) ~[somemod-1.2.3.jar!/:1.2.3]\nCaused by: java.lang.NullPointerException: Cannot invoke \"net.minecraft.world.level.Level.m_5776_()\" because \"level\" is null\n  at com.example.somemod.SomeMod.tick(somemod-1.2.3.jar!/com/example/somemod/SomeMod.class) ~[somemod-1.2.3.jar!/:1.2.3]\n"
    }

    #[test]
    fn mixin_wrapper_is_classified_and_chain_walked() {
        let analysis = analyze_crash(&CrashInputs {
            crash_report: fixture_mixin_crash(),
            ..Default::default()
        });
        assert_eq!(analysis.classification, CrashClassification::GameMixinCrash);
        assert_eq!(analysis.wrapper_exception, "MixinTransformerError");
        assert!(analysis.root_exception.contains("NullPointerException"));
        assert!(analysis
            .mixin_config
            .as_deref()
            .is_some_and(|value| value.contains("some_mod.mixins.json")));
        assert!(analysis
            .mixin_class
            .as_deref()
            .is_some_and(|value| value.contains("SomeMixin")));
        assert!(analysis
            .target_class
            .as_deref()
            .is_some_and(|value| value.contains("LivingEntity")));
    }

    #[test]
    fn owning_jar_mapped_with_evidence_and_confidence() {
        let analysis = analyze_crash(&CrashInputs {
            crash_report: fixture_mixin_crash(),
            mods: vec![ModJarInfo {
                file_name: "somemod-1.2.3.jar".to_string(),
                mod_id: "somemod".to_string(),
                version: "1.2.3".to_string(),
                mixin_configs: vec!["some_mod.mixins.json".to_string()],
            }],
            ..Default::default()
        });
        assert_eq!(analysis.suspected_mods.len(), 1);
        let suspect = &analysis.suspected_mods[0];
        assert_eq!(suspect.mod_id, "somemod");
        assert_eq!(suspect.jar, "somemod-1.2.3.jar");
        assert!(suspect.confidence >= 0.9);
        assert!(!suspect.evidence.is_empty());
    }

    #[test]
    fn no_false_certainty_when_evidence_missing() {
        let analysis = analyze_crash(&CrashInputs {
            crash_report: fixture_mixin_crash(),
            mods: vec![ModJarInfo {
                file_name: "othermod-9.9.jar".to_string(),
                mod_id: "othermod".to_string(),
                version: "9.9".to_string(),
                mixin_configs: vec!["other.mixins.json".to_string()],
            }],
            ..Default::default()
        });
        assert!(analysis.suspected_mods.is_empty());
        assert!(analysis
            .repair_actions
            .iter()
            .any(|action| action.contains("不足以唯一定位")));
    }

    #[test]
    fn mod_loader_crash_classified_separately() {
        let analysis = analyze_crash(&CrashInputs {
            crash_report: "Description: Mod loading error has occurred\njava.lang.Exception: Mod Loading has failed\nCaused by: net.minecraftforge.fml.loading.moddiscovery.InvalidModFileException\n"
                .to_string(),
            ..Default::default()
        });
        assert_eq!(
            analysis.classification,
            CrashClassification::GameModLoaderCrash
        );
    }

    #[test]
    fn index_jar_parses_mods_toml_and_mixin_configs() {
        let path = std::env::temp_dir().join(format!("sh-test-{}.jar", std::process::id()));
        {
            let file = std::fs::File::create(&path).expect("create");
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();
            writer
                .start_file("META-INF/mods.toml", options)
                .expect("start");
            writer
                .write_all(
                    b"modLoader=\"javafml\"\n[[mods]]\nmodId=\"testmod\"\nversion=\"1.0.0\"\n",
                )
                .expect("write toml");
            writer
                .start_file("testmod.mixins.json", options)
                .expect("start");
            writer.write_all(b"{}").expect("write mixins");
            writer.finish().expect("finish");
        }
        let info = index_jar(&path);
        let _ = std::fs::remove_file(&path);
        assert_eq!(info.mod_id, "testmod");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.mixin_configs, vec!["testmod.mixins.json"]);
    }
}
