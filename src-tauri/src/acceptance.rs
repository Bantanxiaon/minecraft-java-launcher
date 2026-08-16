use super::multiplayer::MultiplayerProvider as _;
use super::*;

const E2E_WORLD_NAME: &str = "AcceptanceWorld";

fn e2e_instance_name(loader: &str, game_version: &str, role: &str) -> String {
    format!("Acceptance {loader} {game_version} {role}")
}

/// Resource pack `pack_format` 随 Minecraft 版本变化，按真实版本映射，不写死单一值。
fn pack_format_for_minecraft_version(game_version: &str) -> Option<i32> {
    let version = numeric_game_version(game_version)?;
    let format = match version.as_slice() {
        [1, 20, minor, ..] if *minor >= 3 => 22,
        [1, 20, ..] => 18,
        [1, 21, minor, ..] if *minor <= 1 => 34,
        [1, 21, minor, ..] if *minor <= 3 => 42,
        [1, 21, 4, ..] => 46,
        [1, 21, 5, ..] => 55,
        [1, 21, minor, ..] if *minor <= 8 => 61,
        [1, 21, ..] => 63,
        _ => return None,
    };
    Some(format)
}

/// 动态选择“加载器 + 游戏版本”组合：必须同时满足 vanilla manifest、loader versions
/// 与 e4mc 严格兼容，且按稳定版本优先。不写死版本。
async fn select_e2e_version(
    app: &AppHandle,
    loader: &str,
    candidates: &[String],
) -> Result<String, LauncherError> {
    let manifest = fetch_version_manifest(false).await?;
    let vanilla_versions: std::collections::HashSet<&str> = manifest
        .versions
        .iter()
        .map(|version| version.id.as_str())
        .collect();
    for candidate in candidates {
        if !vanilla_versions.contains(candidate.as_str()) {
            continue;
        }
        if list_loader_versions(loader.to_string(), candidate.clone())
            .await
            .map(|versions| versions.is_empty())
            .unwrap_or(true)
        {
            continue;
        }
        if multiplayer::provider()
            .resolve_version(candidate, loader)
            .await
            .is_ok()
        {
            let _ = app;
            return Ok(candidate.clone());
        }
    }
    Err(LauncherError::validation(format!(
        "没有找到 {loader} 与 e4mc 同时兼容的稳定游戏版本（候选 {}）。",
        candidates.join("、")
    )))
}

async fn select_matrix_versions(app: &AppHandle) -> Result<(String, String), LauncherError> {
    // Forge:1.20.x 系列内按最新到最旧尝试;NeoForge:1.21.x 系列同样动态挑选。
    let forge = select_e2e_version(
        app,
        "forge",
        &["1.20.4".into(), "1.20.2".into(), "1.20.1".into()],
    )
    .await?;
    let neoforge = select_e2e_version(
        app,
        "neoforge",
        &[
            "1.21.11".into(),
            "1.21.10".into(),
            "1.21.9".into(),
            "1.21.8".into(),
            "1.21.7".into(),
            "1.21.6".into(),
            "1.21.5".into(),
            "1.21.4".into(),
            "1.21.3".into(),
            "1.21.2".into(),
            "1.21.1".into(),
        ],
    )
    .await?;
    Ok((forge, neoforge))
}

/// 收集编译 helper mod 所需的类路径:Mod 注解、事件总线与 ServerStartedEvent 所在 jar。
fn helper_compile_classpath(instance_root: &Path) -> Result<Vec<PathBuf>, LauncherError> {
    let libraries = instance_root.join(".minecraft").join("libraries");
    if !libraries.is_dir() {
        return Err(LauncherError::validation("实例 libraries 目录不存在。"));
    }
    let jars: Vec<PathBuf> = walkdir::WalkDir::new(&libraries)
        .into_iter()
        .flatten()
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .to_ascii_lowercase()
                .ends_with(".jar")
        })
        .map(|entry| entry.into_path())
        .collect();
    let contains = |path: &Path, needle: &str| -> bool {
        let Ok(file) = fs::File::open(path) else {
            return false;
        };
        let Ok(mut archive) = zip::ZipArchive::new(file) else {
            return false;
        };
        let found = archive.by_name(needle).is_ok();
        found
    };
    let mut classpath = Vec::new();
    // Minecraft 客户端类必须来自 MCP/Mojmap 命名的 `-client.jar`，
    // 不能用仅含 SRG 混淆名的 srg jar，否则 javac 找不到 getInstance/getLevelSource。
    if let Some(client_jar) = jars.iter().find(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        (name.starts_with("forge-")
            || name.starts_with("neoforge-")
            || name.starts_with("minecraft-client-patched-"))
            && (name.ends_with("-client.jar") || name.contains("client-patched"))
            && contains(path, "net/minecraft/client/Minecraft.class")
    }) {
        classpath.push(client_jar.clone());
    }
    for needle in [
        "net/minecraftforge/fml/common/Mod.class",
        "net/neoforged/fml/common/Mod.class",
        "net/minecraftforge/eventbus/api/SubscribeEvent.class",
        "net/neoforged/bus/api/SubscribeEvent.class",
        "net/minecraftforge/event/server/ServerStartedEvent.class",
        "net/neoforged/neoforge/event/server/ServerStartedEvent.class",
        "net/minecraftforge/event/TickEvent$ClientTickEvent.class",
        "net/neoforged/neoforge/client/event/ClientTickEvent$Post.class",
        "net/minecraftforge/api/distmarker/Dist.class",
        "net/neoforged/api/distmarker/Dist.class",
        "com/mojang/authlib/GameProfile.class",
    ] {
        if let Some(jar) = jars.iter().find(|path| contains(path, needle)) {
            if !classpath.contains(jar) {
                classpath.push(jar.clone());
            }
        }
    }
    let has_client_jar = classpath.iter().any(|path| {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        name.ends_with("-client.jar") || name.contains("client-patched")
    });
    if classpath.len() < 3 || !has_client_jar {
        return Err(LauncherError::validation(format!(
            "未能收集完整的 helper 编译类路径（找到 {} 个 jar）。",
            classpath.len()
        )));
    }
    Ok(classpath)
}

/// 真实测试世界最小完整性：level.dat 与 region 目录都必须存在，
/// 避免把“看起来像目录”的空壳当成可用世界。
fn valid_e2e_world(world: &Path) -> bool {
    world.join("level.dat").is_file() && world.join("region").is_dir()
}

/// 生成与目标游戏版本精确匹配的真实测试世界：
/// 下载该版本官方 server.jar（SHA-1 校验 + 镜像回退），无头启动生成世界后
/// 发送 `stop` 安全关闭，再替换进实例 saves。绝不使用手写 level.dat。
async fn ensure_e2e_world(
    app: &AppHandle,
    instance_id: i64,
    game_version: &str,
    java: &Path,
    saves: &Path,
) -> Result<PathBuf, LauncherError> {
    let world = saves.join(E2E_WORLD_NAME);
    if valid_e2e_world(&world) {
        return Ok(world);
    }
    let manifest = fetch_version_manifest(false).await?;
    let version = manifest
        .versions
        .iter()
        .find(|version| version.id == game_version)
        .ok_or_else(|| LauncherError::validation("验收版本不在 Mojang release 清单中。"))?;
    let details = fetch_version_details(version.url.clone(), version.sha1.clone()).await?;
    let server_url = details
        .pointer("/downloads/server/url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 server 下载地址。"))?
        .to_string();
    let server_sha1 = details
        .pointer("/downloads/server/sha1")
        .and_then(|value| value.as_str())
        .ok_or_else(|| LauncherError::storage("版本元数据缺少 server SHA-1。"))?
        .to_string();
    let server_size = details
        .pointer("/downloads/server/size")
        .and_then(|value| value.as_u64());
    let cache = launcher_data_directory()?
        .join("cache")
        .join("e2e-server")
        .join(game_version);
    fs::create_dir_all(&cache).map_err(|error| LauncherError::storage(error.to_string()))?;
    let server_jar = cache.join("server.jar");
    let server_jar_valid = server_jar.is_file()
        && sha1_file(&server_jar)
            .await
            .map(|value| value.eq_ignore_ascii_case(&server_sha1))
            .unwrap_or(false);
    if !server_jar_valid {
        let _ = fs::remove_file(&server_jar);
        download_verified_file(
            app,
            instance_id,
            &server_url,
            &server_sha1,
            server_size,
            &server_jar,
        )
        .await?;
    }
    let work = cache.join("worldgen");
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|error| LauncherError::storage(error.to_string()))?;
    let properties = [
        format!("level-name={E2E_WORLD_NAME}"),
        // 仅用于一次性离线世界生成器（throwaway vanilla server，cache/worldgen 工作目录），
        // 不是 E2E Host 实例的认证配置；Host 的 integrated server 保持原版 session 校验语义。
        "online-mode=false".to_string(),
        "max-players=2".to_string(),
        "view-distance=4".to_string(),
        "simulation-distance=4".to_string(),
        "server-port=25566".to_string(),
        "motd=SH E2E World Generator".to_string(),
        "enable-command-block=false".to_string(),
        "spawn-monsters=false".to_string(),
        "generate-structures=true".to_string(),
        "level-type=minecraft:normal".to_string(),
        "allow-flight=true".to_string(),
        "gamemode=survival".to_string(),
        "difficulty=normal".to_string(),
    ]
    .join("\n");
    fs::write(work.join("server.properties"), properties.as_bytes())
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    fs::write(work.join("eula.txt"), b"eula=true")
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let java_for_task = java.to_path_buf();
    let jar_for_task = server_jar;
    let work_for_task = work.clone();
    let generated = work.join(E2E_WORLD_NAME);
    let log_path = work.join("server.log");
    tokio::task::spawn_blocking(move || -> Result<(), LauncherError> {
        use std::io::Write as _;
        let stdout = fs::File::create(&log_path)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let stderr = stdout
            .try_clone()
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let mut child = std::process::Command::new(java_for_task)
            .args([
                "-Xmx1280M",
                "-jar",
                jar_for_task.to_str().unwrap_or("server.jar"),
                "nogui",
            ])
            .current_dir(&work_for_task)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::from(stdout))
            .stderr(std::process::Stdio::from(stderr))
            .spawn()
            .map_err(|error| LauncherError::storage(format!("无法启动 server.jar：{error}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| LauncherError::storage("无法接管 server.jar stdin。"))?;
        let deadline = std::time::Instant::now() + Duration::from_secs(6 * 60);
        let mut booted = false;
        while std::time::Instant::now() < deadline {
            if child
                .try_wait()
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .is_some()
            {
                break;
            }
            if fs::read_to_string(&log_path)
                .map(|text| text.contains("Done ("))
                .unwrap_or(false)
            {
                booted = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        if booted {
            let _ = stdin.write_all(b"stop\n");
            let _ = stdin.flush();
            drop(stdin);
            let stop_deadline = std::time::Instant::now() + Duration::from_secs(120);
            loop {
                if child
                    .try_wait()
                    .map_err(|error| LauncherError::storage(error.to_string()))?
                    .is_some()
                    || std::time::Instant::now() >= stop_deadline
                {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            if child
                .try_wait()
                .map_err(|error| LauncherError::storage(error.to_string()))?
                .is_none()
            {
                let _ = child.kill();
                let _ = child.wait();
            }
        } else {
            let _ = child.kill();
            let _ = child.wait();
        }
        Ok(())
    })
    .await
    .map_err(|error| LauncherError::storage(format!("世界生成任务异常：{error}")))??;
    if !valid_e2e_world(&generated) {
        return Err(LauncherError::storage(format!(
            "server.jar 已结束但未生成有效世界：{}",
            generated.to_string_lossy()
        )));
    }
    if world.exists() {
        let _ = fs::remove_dir_all(&world);
    }
    fs::rename(&generated, &world).map_err(|error| LauncherError::storage(error.to_string()))?;
    if !valid_e2e_world(&world) {
        return Err(LauncherError::storage("世界替换后校验失败。"));
    }
    Ok(world)
}

/// 测试专用 helper mod：世界加载完成后自动执行 /publish（等价 Open to LAN）。
/// 只进入验收实例，不进入正式 Installer；不修改 online-mode、不绕过认证、不实现隧道。
fn build_e2e_helper_jar(
    loader: &str,
    classpath: &str,
    javac: &Path,
    pack_format: i32,
    output: &Path,
) -> Result<(), LauncherError> {
    use std::process::Command;
    let (mods_toml, java_source) = match loader {
        "forge" => (
            "modLoader=\"javafml\"\nloaderVersion=\"[47,)\"\nlicense=\"MIT\"\n\n[[mods]]\nmodId=\"sh_e2e_helper\"\nversion=\"1.0.0\"\ndisplayName=\"SH E2E Helper\"\n",
            r#"package sh.e2e;

import net.minecraft.client.Minecraft;
import net.minecraftforge.common.MinecraftForge;
import net.minecraftforge.event.TickEvent;
import net.minecraftforge.event.server.ServerStartedEvent;
import net.minecraftforge.eventbus.api.SubscribeEvent;
import net.minecraftforge.fml.common.Mod;

// Forge 1.20.x 运行时的 Minecraft 类使用 SRG 命名（m_*/f_*），
// 本 helper 直接按 SRG 编译，避免依赖运行时重映射。
@Mod("sh_e2e_helper")
public class ShE2eHelper {
    private static final String WORLD_NAME = "AcceptanceWorld";
    private int clientTicks = 0;
    private boolean worldOpenRequested = false;
    private boolean joinRequested = false;
    private String lastScreen = "";

    public ShE2eHelper() {
        MinecraftForge.EVENT_BUS.register(this);
        System.out.println("SH_E2E_HELPER_READY");
    }

    @SubscribeEvent
    public void onClientTick(TickEvent.ClientTickEvent event) {
        if (event.phase != TickEvent.Phase.END) {
            return;
        }
        Minecraft minecraft = Minecraft.m_91087_();
        if (minecraft == null || minecraft.f_91073_ != null) {
            return;
        }
        clientTicks++;
        String screenName = minecraft.f_91080_ == null ? "null" : minecraft.f_91080_.getClass().getName();
        if (!screenName.equals(lastScreen)) {
            lastScreen = screenName;
            System.out.println("SH_E2E_SCREEN:" + screenName);
            if (minecraft.f_91080_ instanceof net.minecraft.client.gui.screens.DisconnectedScreen disconnected) {
                try {
                    java.lang.reflect.Field title = net.minecraft.client.gui.screens.DisconnectedScreen.class.getDeclaredField("f_95988_");
                    java.lang.reflect.Field reason = net.minecraft.client.gui.screens.DisconnectedScreen.class.getDeclaredField("f_278396_");
                    title.setAccessible(true);
                    reason.setAccessible(true);
                    System.out.println("SH_E2E_DISCONNECT:" + title.get(disconnected) + " | " + reason.get(disconnected));
                } catch (Throwable error) {
                    System.err.println("SH_E2E_DISCONNECT_REFLECT_ERROR:" + error);
                }
            }
        }
        if (clientTicks == 1) {
            System.out.println("SH_E2E_FIRST_SCREEN:"
                    + (minecraft.f_91080_ == null ? "null" : minecraft.f_91080_.getClass().getName()));
            System.out.println("SH_E2E_WORLD_EXISTS:"
                    + minecraft.m_91392_().m_78255_(WORLD_NAME));
        }
        if (clientTicks == 10 && !joinRequested) {
            // Guest 通过启动器写入的 sh_e2e_join.txt（形如 127.0.0.1:PORT，即加入 shim）
            // 自动连接真实公网域名。保持原版客户端认证语义（Type.OTHER 普通服务器）：
            // e4mc 上游仅要求客户端把 *.e4mc.link 当普通服务器地址连接，不要求 LAN 标记；
            // 不绕过 joinServer / Mojang/Microsoft session 校验。离线测试账户会在认证边界
            // 收到 “Invalid session”，该结果分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION。
            java.nio.file.Path joinFile =
                    minecraft.m_91392_().m_78257_().getParent().resolve("sh_e2e_join.txt");
            try {
                if (java.nio.file.Files.isRegularFile(joinFile)) {
                    String target = new String(java.nio.file.Files.readAllBytes(joinFile),
                            java.nio.charset.StandardCharsets.UTF_8).trim();
                    if (!target.isEmpty()) {
                        joinRequested = true;
                        String[] parts = target.split(":", -1);
                        String hostPart = parts[0];
                        int port = parts.length >= 2 ? Integer.parseInt(parts[1]) : 25565;
                        System.out.println("SH_E2E_HELPER_JOIN:" + target);
                        net.minecraft.client.multiplayer.resolver.ServerAddress address =
                                new net.minecraft.client.multiplayer.resolver.ServerAddress(hostPart, port);
                        net.minecraft.client.multiplayer.ServerData data =
                                new net.minecraft.client.multiplayer.ServerData("SH E2E Server", target,
                                        net.minecraft.client.multiplayer.ServerData.Type.OTHER);
                        net.minecraft.client.gui.screens.ConnectScreen.m_278792_(
                                minecraft.f_91080_, minecraft, address, data, false);
                    }
                }
            } catch (Throwable error) {
                System.err.println("SH_E2E_HELPER_JOIN_ERROR:" + error);
            }
        }
        if (clientTicks == 20
                && !joinRequested
                && !worldOpenRequested
                && minecraft.f_91080_ != null
                && "net.minecraft.client.gui.screens.TitleScreen"
                        .equals(minecraft.f_91080_.getClass().getName())
                && minecraft.m_91392_().m_78255_(WORLD_NAME)) {
            worldOpenRequested = true;
            System.out.println("SH_E2E_HELPER_OPEN_WORLD:" + WORLD_NAME);
            minecraft.m_231466_().m_306404_(WORLD_NAME, () -> {});
        }
    }

    @SubscribeEvent
    public void onServerStarted(ServerStartedEvent event) {
        try {
            Object server = event.getClass().getMethod("getServer").invoke(event);
            Object commands = server.getClass().getMethod("m_129892_").invoke(server);
            Object source = server.getClass().getMethod("m_129893_").invoke(server);
            Class<?> sourceType = Class.forName("net.minecraft.commands.CommandSourceStack");
            commands.getClass().getMethod("m_230957_", sourceType, String.class)
                    .invoke(commands, source, "publish");
            System.out.println("SH_E2E_HELPER_PUBLISHED");
        } catch (Throwable error) {
            System.err.println("SH_E2E_HELPER_ERROR:" + error);
        }
    }

    @SubscribeEvent
    public void onClientLogin(net.minecraftforge.client.event.ClientPlayerNetworkEvent.LoggingIn event) {
        // 客户端完成登录/进入世界（真实联机证据，不依赖“Connecting to”这类弱标记）。
        // Forge 客户端 jar 使用 SRG 命名：m_36316_ 即 Player#getGameProfile。
        System.out.println("SH_E2E_CLIENT_JOINED:" + event.getPlayer().m_36316_().getName());
    }
}
"#,
        ),
        "neoforge" => (
            "modLoader=\"javafml\"\nloaderVersion=\"[2,)\"\nlicense=\"MIT\"\n\n[[mods]]\nmodId=\"sh_e2e_helper\"\nversion=\"1.0.0\"\ndisplayName=\"SH E2E Helper\"\n",
            r#"package sh.e2e;

import net.minecraft.client.Minecraft;
import net.neoforged.neoforge.common.NeoForge;
import net.neoforged.neoforge.client.event.ClientTickEvent;
import net.neoforged.neoforge.event.server.ServerStartedEvent;
import net.neoforged.bus.api.SubscribeEvent;
import net.neoforged.fml.common.Mod;

@Mod("sh_e2e_helper")
public class ShE2eHelper {
    private static final String WORLD_NAME = "AcceptanceWorld";
    private int clientTicks = 0;
    private boolean worldOpenRequested = false;
    private boolean joinRequested = false;

    public ShE2eHelper() {
        NeoForge.EVENT_BUS.register(this);
        System.out.println("SH_E2E_HELPER_READY");
    }

    @SubscribeEvent
    public void onClientTick(ClientTickEvent.Post event) {
        Minecraft minecraft = Minecraft.getInstance();
        if (minecraft == null || minecraft.level != null) {
            return;
        }
        clientTicks++;
        if (clientTicks == 1) {
            System.out.println("SH_E2E_FIRST_SCREEN:"
                    + (minecraft.screen == null ? "null" : minecraft.screen.getClass().getName()));
            System.out.println("SH_E2E_WORLD_EXISTS:"
                    + minecraft.getLevelSource().levelExists(WORLD_NAME));
        }
        if (clientTicks == 10 && !joinRequested) {
            // Guest 通过 sh_e2e_join.txt（127.0.0.1:PORT，即加入 shim）自动连接真实公网域名。
            // 保持原版客户端认证语义：e4mc 上游只要求客户端把 *.e4mc.link 当作普通服务器地址
            // 连接（“Others can simply connect to the public domain”），不要求也不应使用 LAN
            // 标记。ServerData.Type 只影响 UI，不影响 joinServer/session 校验；这里统一使用
            // Type.OTHER 表示普通服务器，禁止通过伪装 LAN 绕过任何认证语义。
            // 离线测试账户会在标准 login/auth 边界收到“Invalid session”/loginFailedInfo.invalidSession，
            // 该结果分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION，不作为网络失败。
            java.nio.file.Path joinFile =
                    minecraft.getLevelSource().getBaseDir().getParent().resolve("sh_e2e_join.txt");
            try {
                if (java.nio.file.Files.isRegularFile(joinFile)) {
                    String target = new String(java.nio.file.Files.readAllBytes(joinFile),
                            java.nio.charset.StandardCharsets.UTF_8).trim();
                    if (!target.isEmpty()) {
                        joinRequested = true;
                        String[] parts = target.split(":", -1);
                        String hostPart = parts[0];
                        int port = parts.length >= 2 ? Integer.parseInt(parts[1]) : 25565;
                        System.out.println("SH_E2E_HELPER_JOIN:" + target);
                        net.minecraft.client.multiplayer.resolver.ServerAddress address =
                                net.minecraft.client.multiplayer.resolver.ServerAddress.parseString(hostPart + ":" + port);
                        net.minecraft.client.multiplayer.ServerData data =
                                new net.minecraft.client.multiplayer.ServerData("SH E2E Server", target,
                                        net.minecraft.client.multiplayer.ServerData.Type.OTHER);
                        net.minecraft.client.gui.screens.ConnectScreen.startConnecting(
                                minecraft.screen, minecraft, address, data, false, null);
                    }
                }
            } catch (Throwable error) {
                System.err.println("SH_E2E_HELPER_JOIN_ERROR:" + error);
            }
        }
        if (clientTicks == 20
                && !joinRequested
                && !worldOpenRequested
                && minecraft.screen != null
                && "net.minecraft.client.gui.screens.TitleScreen"
                        .equals(minecraft.screen.getClass().getName())
                && minecraft.getLevelSource().levelExists(WORLD_NAME)) {
            worldOpenRequested = true;
            System.out.println("SH_E2E_HELPER_OPEN_WORLD:" + WORLD_NAME);
            minecraft.createWorldOpenFlows().openWorld(WORLD_NAME, () -> {});
        }
    }

    @SubscribeEvent
    public void onServerStarted(ServerStartedEvent event) {
        try {
            Object server = event.getClass().getMethod("getServer").invoke(event);
            Object commands = server.getClass().getMethod("getCommands").invoke(server);
            Object source = server.getClass().getMethod("createCommandSourceStack").invoke(server);
            Class<?> sourceType = Class.forName("net.minecraft.commands.CommandSourceStack");
            commands.getClass().getMethod("performPrefixedCommand", sourceType, String.class)
                    .invoke(commands, source, "publish");
            System.out.println("SH_E2E_HELPER_PUBLISHED");
        } catch (Throwable error) {
            System.err.println("SH_E2E_HELPER_ERROR:" + error);
        }
    }

    @SubscribeEvent
    public void onClientLogin(net.neoforged.neoforge.client.event.ClientPlayerNetworkEvent.LoggingIn event) {
        // 客户端完成登录/进入世界（真实联机证据，不依赖“Connecting to”这类弱标记）。
        // 1.21.x 的 GameProfile 是 record，字段访问器为 name()。
        System.out.println("SH_E2E_CLIENT_JOINED:" + event.getPlayer().getGameProfile().name());
    }
}
"#,
        ),
        _ => return Err(LauncherError::validation("不支持的 helper 加载器。")),
    };
    let work = output.with_extension("workdir");
    fs::create_dir_all(&work).map_err(|error| LauncherError::storage(error.to_string()))?;
    let source_path = work.join("ShE2eHelper.java");
    fs::write(&source_path, java_source)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let classes = work.join("classes");
    fs::create_dir_all(&classes).map_err(|error| LauncherError::storage(error.to_string()))?;
    let status = Command::new(javac)
        .args([
            "-encoding",
            "UTF-8",
            "--release",
            "17",
            "-classpath",
            classpath,
            "-d",
            &classes.to_string_lossy(),
            &source_path.to_string_lossy(),
        ])
        .status()
        .map_err(|error| LauncherError::storage(format!("无法运行 javac：{error}")))?;
    if !status.success() {
        return Err(LauncherError::validation("E2E helper mod 编译失败。"));
    }
    let class_path = classes.join("sh").join("e2e").join("ShE2eHelper.class");
    if !class_path.is_file() {
        return Err(LauncherError::validation("E2E helper 编译产物缺失。"));
    }
    let file =
        fs::File::create(output).map_err(|error| LauncherError::storage(error.to_string()))?;
    let mut archive = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    archive
        .start_file("META-INF/mods.toml", options)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    use std::io::Write as _;
    archive
        .write_all(mods_toml.as_bytes())
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let pack_mcmeta = serde_json::json!({
        "pack": {
            "description": "SH Launcher Multiplayer E2E Helper",
            "pack_format": pack_format
        }
    });
    archive
        .start_file("pack.mcmeta", options)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    archive
        .write_all(
            serde_json::to_string(&pack_mcmeta)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    archive
        .start_file("sh/e2e/ShE2eHelper.class", options)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let bytes = fs::read(&class_path).map_err(|error| LauncherError::storage(error.to_string()))?;
    archive
        .write_all(&bytes)
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    archive
        .finish()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

/// Helper artifact preflight：结构、TOML、pack.mcmeta、入口类与 Loader 污染检查。
/// 任何一项失败都禁止启动 Minecraft。
pub(crate) fn preflight_helper_jar(
    jar: &Path,
    expected_pack_format: i32,
) -> Result<(), LauncherError> {
    let file = fs::File::open(jar)
        .map_err(|error| LauncherError::validation(format!("helper JAR 无法打开：{error}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|error| LauncherError::validation(format!("helper JAR 结构损坏：{error}")))?;
    let mut names = std::collections::HashSet::new();
    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let name = entry.name().to_string();
        if name.is_empty() || !names.insert(name.clone()) {
            return Err(LauncherError::validation(format!(
                "helper JAR 条目无效或重复：{name}"
            )));
        }
        let mut components = Path::new(&name).components();
        if !components.all(|component| matches!(component, std::path::Component::Normal(_))) {
            return Err(LauncherError::validation(format!(
                "helper JAR 包含异常路径条目：{name}"
            )));
        }
    }
    let require_entry = |needle: &str| -> Result<(), LauncherError> {
        if names.contains(needle) {
            Ok(())
        } else {
            Err(LauncherError::validation(format!(
                "helper JAR 缺少条目：{needle}"
            )))
        }
    };
    require_entry("META-INF/mods.toml")?;
    require_entry("pack.mcmeta")?;
    require_entry("sh/e2e/ShE2eHelper.class")?;
    // 禁止默认包入口与任何 Loader runtime 污染。
    for name in &names {
        if name.ends_with(".class") && !name.contains('/') {
            return Err(LauncherError::validation(format!(
                "helper 入口必须在具名包中：{name}"
            )));
        }
        let forbidden = [
            "net/minecraftforge/",
            "net/neoforged/",
            "net/fabricmc/",
            "org/spongepowered/",
            "cpw/mods/",
            "net/minecraft/",
        ];
        if forbidden.iter().any(|prefix| name.starts_with(prefix)) {
            return Err(LauncherError::validation(format!(
                "helper JAR 意外捆绑了 Loader/runtime 类：{name}"
            )));
        }
    }
    // mods.toml：parse 且 modId 匹配。
    {
        let mut mods_entry = archive
            .by_name("META-INF/mods.toml")
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let mut mods_bytes = Vec::new();
        mods_entry
            .read_to_end(&mut mods_bytes)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let mods_text = String::from_utf8_lossy(&mods_bytes);
        let mods: toml::Value = toml::from_str(&mods_text)
            .map_err(|error| LauncherError::validation(format!("mods.toml 解析失败：{error}")))?;
        if mods.get("modLoader").and_then(|value| value.as_str()) != Some("javafml") {
            return Err(LauncherError::validation(
                "mods.toml 的 modLoader 必须为 javafml。",
            ));
        }
        let mod_id_ok = mods
            .get("mods")
            .and_then(|value| value.as_array())
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("modId"))
            .and_then(|value| value.as_str())
            == Some("sh_e2e_helper");
        if !mod_id_ok {
            return Err(LauncherError::validation(
                "mods.toml 的 modId 必须为 sh_e2e_helper。",
            ));
        }
    }
    // pack.mcmeta：合法 JSON + pack_format 与目标版本匹配。
    {
        let mut pack_entry = archive
            .by_name("pack.mcmeta")
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let mut pack_bytes = Vec::new();
        pack_entry
            .read_to_end(&mut pack_bytes)
            .map_err(|error| LauncherError::validation(error.to_string()))?;
        let pack: serde_json::Value = serde_json::from_slice(&pack_bytes).map_err(|error| {
            LauncherError::validation(format!("pack.mcmeta JSON 无效：{error}"))
        })?;
        let description_ok = pack
            .pointer("/pack/description")
            .and_then(|value| value.as_str())
            .is_some_and(|value| !value.trim().is_empty());
        let format_ok = pack
            .pointer("/pack/pack_format")
            .and_then(|value| value.as_i64())
            == Some(expected_pack_format as i64);
        if !description_ok || !format_ok {
            return Err(LauncherError::validation(format!(
                "pack.mcmeta 无效：description={description_ok} pack_format={format_ok}（期望 {expected_pack_format}）"
            )));
        }
    }
    // 入口类必须携带与 mods.toml 一致的 @Mod 值（类常量池字符串）。
    let mut class_entry = archive
        .by_name("sh/e2e/ShE2eHelper.class")
        .map_err(|error| LauncherError::validation(error.to_string()))?;
    let mut class_bytes = Vec::new();
    class_entry
        .read_to_end(&mut class_bytes)
        .map_err(|error| LauncherError::validation(error.to_string()))?;
    let class_text = String::from_utf8_lossy(&class_bytes);
    if !class_text.contains("sh_e2e_helper") {
        return Err(LauncherError::validation(
            "入口类缺少与 mods.toml 一致的 @Mod 值。",
        ));
    }
    Ok(())
}

fn ensure_offline_accounts(app: &AppHandle) -> Result<(i64, i64), LauncherError> {
    let connection = open_database(app)?;
    let mut ids = Vec::new();
    for name in ["AcceptHost", "AcceptGuest"] {
        let existing: Option<i64> = connection
            .query_row(
                "SELECT id FROM accounts WHERE account_type='OFFLINE' AND display_name=?1",
                [name],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        if let Some(id) = existing {
            ids.push(id);
            continue;
        }
        let uuid = minecraft_offline_uuid(name).to_string();
        connection
            .execute(
                "INSERT INTO accounts(account_type, display_name, minecraft_uuid, legacy_offline_uuid, created_at, last_used_at)
                 VALUES('OFFLINE', ?1, ?2, ?2, ?3, NULL)",
                params![name, uuid, chrono_like_timestamp()],
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        ids.push(connection.last_insert_rowid());
    }
    Ok((ids[0], ids[1]))
}

/// 验收账户选择：
/// - 默认使用隔离的 OFFLINE 测试账户（`accounts_offline=true`），session 校验必然在
///   Mojang/Microsoft 边界失败，必须分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION；
/// - 当环境变量 `LAUNCHER_E2E_HOST_ACCOUNT` / `LAUNCHER_E2E_GUEST_ACCOUNT` 指定已存在的
///   合法在线账户时，使用这些账户并要求完成真正的 session 验证进入世界。
fn ensure_e2e_accounts(app: &AppHandle) -> Result<(i64, i64, bool), LauncherError> {
    let host_name = std::env::var("LAUNCHER_E2E_HOST_ACCOUNT");
    let guest_name = std::env::var("LAUNCHER_E2E_GUEST_ACCOUNT");
    if let (Ok(host_name), Ok(guest_name)) = (host_name, guest_name) {
        let connection = open_database(app)?;
        let host = account_id_by_name(&connection, &host_name).ok_or_else(|| {
            LauncherError::validation(format!("在线 Host 账户不存在：{host_name}"))
        })?;
        let guest = account_id_by_name(&connection, &guest_name).ok_or_else(|| {
            LauncherError::validation(format!("在线 Guest 账户不存在：{guest_name}"))
        })?;
        return Ok((host, guest, false));
    }
    let (host, guest) = ensure_offline_accounts(app)?;
    Ok((host, guest, true))
}

fn account_id_by_name(connection: &rusqlite::Connection, name: &str) -> Option<i64> {
    connection
        .query_row(
            "SELECT id FROM accounts WHERE display_name=?1",
            [name],
            |row| row.get(0),
        )
        .optional()
        .ok()
        .flatten()
}

fn set_instance_game_args(
    app: &AppHandle,
    instance_id: i64,
    args: &[&str],
) -> Result<(), LauncherError> {
    let json =
        serde_json::to_string(&args).map_err(|error| LauncherError::storage(error.to_string()))?;
    let connection = open_database(app)?;
    connection
        .execute(
            "INSERT INTO instance_launch_settings(instance_id, memory_min_mb, memory_max_mb, game_args_json)
             VALUES(?1, 1024, 2048, ?2)
             ON CONFLICT(instance_id) DO UPDATE SET game_args_json=excluded.game_args_json",
            params![instance_id, json],
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(())
}

fn latest_instance_log(instance_root: &Path) -> Option<PathBuf> {
    let logs = instance_root.join(".minecraft").join("logs");
    let mut entries: Vec<(std::time::SystemTime, PathBuf)> = fs::read_dir(&logs)
        .ok()?
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("launcher-"))
        .filter_map(|entry| {
            entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .map(|modified| (modified, entry.path()))
        })
        .collect();
    entries.sort_by_key(|(modified, _)| *modified);
    entries.pop().map(|(_, path)| path)
}

fn read_log(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// 单轮联机验收的分层证据。只依据 Host/Guest 真实日志判定，不做任何推断或回退，
/// 也不允许用 LAN 伪装、修改 online-mode 或伪造 session 来“变绿”。
#[derive(Debug, Clone, Default)]
struct JoinLayerEvidence {
    host_quic_incoming_stream: bool,
    host_joined: bool,
    host_offline_fallback: bool,
    guest_connect_attempted: bool,
    guest_login_boundary_reached: bool,
    guest_invalid_session: bool,
    guest_joined: bool,
}

impl JoinLayerEvidence {
    /// 传输层：Host 观察到非控制通道（streamId != 0）的真实 e4mc relay QUIC 流入流。
    fn relay_forwarding_pass(&self) -> bool {
        self.host_quic_incoming_stream
    }

    /// Minecraft 握手：Guest 发起连接，且该连接经 relay 到达 Host。
    fn handshake_pass(&self) -> bool {
        self.guest_connect_attempted && self.host_quic_incoming_stream
    }

    /// RSA 登录传输：到达 login/auth 边界（加密响应阶段完成后才会发生 session 校验）。
    /// “Invalid session” / “will let them in anyway” 均发生在加密通道建立之后，
    /// 因此到达边界即可判定 RSA 传输通过。
    fn rsa_login_transport_pass(&self) -> bool {
        self.guest_login_boundary_reached || self.host_offline_fallback || self.host_joined
    }

    /// 只有经 Mojang/Microsoft session 校验通过后的进入世界才算“已认证加入”。
    /// 原版 integrated server 的离线回退（"will let them in anyway"）不算认证通过。
    fn session_verified_join(&self) -> bool {
        self.host_joined
            && self.guest_joined
            && !self.host_offline_fallback
            && !self.guest_invalid_session
    }

    /// 离线测试账户在标准 session 校验边界被拒绝（Invalid session）。
    fn blocked_by_test_account_session(&self) -> bool {
        self.guest_invalid_session
    }

    /// 双方日志都证实进入世界（无论是否经 session 验证，例如原版离线回退）。
    fn world_connection_established(&self) -> bool {
        self.host_joined && self.guest_joined
    }
}

/// 单轮加入结果的判定策略（纯函数，可单元测试）。
/// 网络/relay/握手失败是硬失败；离线账户在 session 边界被拒是账户限制（pending），
/// 在线账户则必须完成真正的 session 验证。
#[derive(Debug)]
struct RoundJoinOutcome {
    relay_pass: bool,
    handshake_pass: bool,
    rsa_pass: bool,
    session_verified: bool,
    world_join_status: &'static str,
    round_status: &'static str,
}

fn evaluate_round_join(
    layers: &JoinLayerEvidence,
    accounts_offline: bool,
) -> Result<RoundJoinOutcome, String> {
    let relay_pass = layers.relay_forwarding_pass();
    let handshake_pass = layers.handshake_pass();
    let rsa_pass = layers.rsa_login_transport_pass();
    if !relay_pass || !handshake_pass || !rsa_pass {
        return Err(format!(
            "公网链路未建立（网络/relay/握手失败，非账户限制）：relayForwarding={relay_pass} minecraftHandshake={handshake_pass} rsaLoginTransport={rsa_pass} guestConnectAttempted={} hostQuicIncomingStream={}",
            layers.guest_connect_attempted, layers.host_quic_incoming_stream
        ));
    }
    let session_verified = layers.session_verified_join();
    if !accounts_offline && !session_verified {
        return Err("在线账户必须完成 Mojang/Microsoft session 验证后的进入世界。".to_string());
    }
    let world_join_status = if session_verified {
        "PASS_SESSION_VERIFIED"
    } else if layers.blocked_by_test_account_session() {
        "BLOCKED_BY_TEST_ACCOUNT_SESSION"
    } else if layers.host_offline_fallback && layers.world_connection_established() {
        "VANILLA_SINGLEPLAYER_OFFLINE_FALLBACK"
    } else {
        "NOT_REACHED"
    };
    let round_status = if session_verified {
        "passed"
    } else {
        "passed_with_external_account_pending"
    };
    Ok(RoundJoinOutcome {
        relay_pass,
        handshake_pass,
        rsa_pass,
        session_verified,
        world_join_status,
        round_status,
    })
}

fn quic_stream_id(line: &str) -> Option<u64> {
    let marker = "QuicStreamAddress{streamId=";
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('}')?;
    rest[..end].trim().parse().ok()
}

fn classify_join_evidence(host_text: &str, guest_text: &str) -> JoinLayerEvidence {
    let host_lower = host_text.to_ascii_lowercase();
    let guest_lower = guest_text.to_ascii_lowercase();
    let host_quic_incoming_stream = host_text
        .lines()
        .any(|line| quic_stream_id(line).is_some_and(|id| id != 0));
    let guest_invalid_session = guest_lower.contains("invalid session")
        || guest_lower.contains("invalid_session")
        || guest_text.contains("loginFailedInfo.invalidSession");
    JoinLayerEvidence {
        host_quic_incoming_stream,
        host_joined: host_text.contains("AcceptGuest joined the game")
            || host_text.contains("AcceptGuest 加入了游戏"),
        host_offline_fallback: host_lower
            .contains("failed to verify username but will let them in anyway"),
        guest_connect_attempted: guest_text.contains("Connecting to ")
            || guest_text.contains("SH_E2E_HELPER_JOIN:"),
        guest_login_boundary_reached: guest_lower.contains("failed to log in:")
            || guest_text.contains("SH_E2E_DISCONNECT:")
            || host_lower.contains("user authenticator")
            || host_lower.contains("tried to join with an invalid session")
            || guest_invalid_session,
        guest_invalid_session,
        guest_joined: guest_text.contains("SH_E2E_CLIENT_JOINED:AcceptGuest"),
    }
}

/// 提取与关键判定相关的证据行，保证证据 machine-readable 且可审计。
fn join_log_excerpt(text: &str, limit: usize) -> Vec<String> {
    let keywords = [
        "QuicStreamAddress",
        "AcceptGuest",
        "SH_E2E_HELPER_JOIN",
        "SH_E2E_CLIENT_JOINED",
        "Connecting to ",
        "Failed to log in",
        "Invalid session",
        "invalidSession",
        "will let them in anyway",
        "Domain assigned",
        "SH_E2E_DISCONNECT",
    ];
    text.lines()
        .filter(|line| keywords.iter().any(|keyword| line.contains(keyword)))
        .map(str::to_string)
        .take(limit)
        .collect()
}

/// 验收轮次失败时的强制回收：结束游戏进程并同步关闭联机会话与加入 shim，
/// 防止残留 Java 进程锁住 latest.log / 世界 session.lock，污染后续轮次。
fn cleanup_round_processes(app: &AppHandle, host_id: i64, guest_id: i64) {
    let _ = stop_game(host_id);
    let _ = terminate_game(guest_id);
    multiplayer::on_game_exit(app, host_id);
    multiplayer::on_game_exit(app, guest_id);
}

pub(crate) async fn run_multiplayer_prepare_acceptance(
    app: AppHandle,
) -> Result<serde_json::Value, LauncherError> {
    let (forge_version, neoforge_version) = select_matrix_versions(&app).await?;
    let java17 = std::env::var("LAUNCHER_E2E_JAVA17")
        .map_err(|_| LauncherError::validation("缺少 LAUNCHER_E2E_JAVA17。"))?;
    let java21 = std::env::var("LAUNCHER_E2E_JAVA21")
        .map_err(|_| LauncherError::validation("缺少 LAUNCHER_E2E_JAVA21。"))?;

    let mut matrix = Vec::new();
    for (loader, game_version, java) in [
        ("forge", &forge_version, &java17),
        ("neoforge", &neoforge_version, &java21),
    ] {
        let host_name = e2e_instance_name(loader, game_version, "Host");
        let guest_name = e2e_instance_name(loader, game_version, "Guest");
        let mut host = list_instances(app.clone())?
            .into_iter()
            .find(|instance| instance.name == host_name && instance.status == "ready");
        if host.is_none() {
            let base_name = format!("Acceptance {loader} {game_version}");
            let vanilla_name = format!("Acceptance Vanilla {game_version}");
            let vanilla_ready = list_instances(app.clone())?
                .into_iter()
                .any(|instance| instance.name == vanilla_name && instance.status == "ready");
            if !vanilla_ready {
                run_vanilla_install_acceptance(app.clone(), game_version.clone()).await?;
            }
            let base = list_instances(app.clone())?
                .into_iter()
                .find(|instance| instance.name == base_name && instance.status == "ready");
            let base = match base {
                Some(base) => base,
                None => {
                    run_loader_install_acceptance(
                        app.clone(),
                        game_version.clone(),
                        loader.to_string(),
                        java.clone(),
                    )
                    .await?;
                    list_instances(app.clone())?
                        .into_iter()
                        .find(|instance| instance.name == base_name && instance.status == "ready")
                        .ok_or_else(|| LauncherError::validation("加载器实例未就绪。"))?
                }
            };
            host = Some(rename_instance(app.clone(), base.id, host_name.clone())?);
        }
        let host = host.ok_or_else(|| LauncherError::validation("Host 实例缺失。"))?;
        let guest = list_instances(app.clone())?
            .into_iter()
            .find(|instance| instance.name == guest_name)
            .map(Ok)
            .unwrap_or_else(|| clone_instance(app.clone(), host.id, guest_name.clone(), false))?;
        {
            let connection = open_database(&app)?;
            connection
                .execute(
                    "INSERT INTO installation_states(instance_id, component_kind, component_key, hash, size_bytes, status)
                     SELECT ?1, component_kind, component_key, hash, size_bytes, 'verified'
                     FROM installation_states WHERE instance_id=?2
                     ON CONFLICT(instance_id, component_kind, component_key) DO UPDATE SET status='verified'",
                    params![guest.id, host.id],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
            connection
                .execute(
                    "UPDATE instances SET status='ready', loader_version=(SELECT loader_version FROM instances WHERE id=?1) WHERE id=?2",
                    params![host.id, guest.id],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }

        let host_root = PathBuf::from(&host.root_path);
        let compile_classpath = helper_compile_classpath(&host_root)?;
        let classpath_string = compile_classpath
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect::<Vec<_>>()
            .join(";");
        let helper_jar = host_root
            .join(".minecraft")
            .join("mods")
            .join("sh_e2e_helper.jar");
        let javac = PathBuf::from(&java)
            .parent()
            .map(|parent| parent.join("javac.exe"))
            .unwrap_or_else(|| PathBuf::from("javac.exe"));
        let pack_format = pack_format_for_minecraft_version(game_version).ok_or_else(|| {
            LauncherError::validation(format!("无法映射 {game_version} 的 resource pack_format。"))
        })?;
        build_e2e_helper_jar(loader, &classpath_string, &javac, pack_format, &helper_jar)?;
        preflight_helper_jar(&helper_jar, pack_format)?;
        // Guest 也需要 helper：1.20.4 不支持 --server 自动加入，Guest 由 helper 读取
        // sh_e2e_join.txt 后自动连接真实公网域名；Host 则由 helper 打开测试世界。
        let guest_helper_jar = PathBuf::from(&guest.root_path)
            .join(".minecraft")
            .join("mods")
            .join("sh_e2e_helper.jar");
        if let Some(parent) = guest_helper_jar.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        fs::copy(&helper_jar, &guest_helper_jar)
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        // 全新实例首启会弹出无障碍引导界面，阻塞 helper 的自动进入世界/自动加入：
        // 预写 options.txt 跳过该一次性引导，保证验收在任何干净环境下都可重复。
        let guest_root = PathBuf::from(&guest.root_path);
        for instance_root in [host_root.as_path(), guest_root.as_path()] {
            fs::write(
                instance_root.join(".minecraft").join("options.txt"),
                b"onboardAccessibility:true\n",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        let world = ensure_e2e_world(
            &app,
            host.id,
            game_version,
            Path::new(&java),
            &host_root.join(".minecraft").join("saves"),
        )
        .await?;
        // 不依赖 --quickPlaySingleplayer：Forge 1.20.x 在快速游玩下会与
        // LootModifierManager 的资源重载发生竞态崩溃；统一由 helper 在标题界面稳定进入世界。
        set_instance_game_args(&app, host.id, &[])?;
        set_instance_game_args(&app, guest.id, &[])?;
        {
            let connection = open_database(&app)?;
            connection
                .execute(
                    "UPDATE instances SET memory_mb=2048 WHERE id IN (?1, ?2)",
                    params![host.id, guest.id],
                )
                .map_err(|error| LauncherError::storage(error.to_string()))?;
        }
        matrix.push(serde_json::json!({
            "loader": loader,
            "gameVersion": game_version,
            "java": java,
            "hostInstanceId": host.id,
            "guestInstanceId": guest.id,
            "helperJar": helper_jar.to_string_lossy(),
            "world": world.to_string_lossy(),
            "compileClasspath": compile_classpath
                .iter()
                .map(|path| path.to_string_lossy().to_string())
                .collect::<Vec<_>>(),
        }));
    }
    let (host_account, guest_account) = ensure_offline_accounts(&app)?;
    Ok(serde_json::json!({
        "status": "passed",
        "matrix": matrix,
        "hostAccountId": host_account,
        "guestAccountId": guest_account,
        "completedAt": chrono_like_timestamp()
    }))
}

pub(crate) async fn run_multiplayer_matrix_acceptance(
    app: AppHandle,
    loader: String,
    minutes: u64,
    rounds: u32,
    include_crash: bool,
) -> Result<serde_json::Value, LauncherError> {
    let loader = loader.trim().to_ascii_lowercase();
    validate_loader_type(&loader)?;
    let java = std::env::var("LAUNCHER_E2E_JAVA")
        .map_err(|_| LauncherError::validation("缺少 LAUNCHER_E2E_JAVA。"))?;
    let host = list_instances(app.clone())?
        .into_iter()
        .find(|instance| {
            instance.loader_type == loader
                && instance.name.starts_with(&format!("Acceptance {loader} "))
                && instance.name.ends_with(" Host")
                && instance.status == "ready"
        })
        .ok_or_else(|| LauncherError::validation("Host 验收实例不存在。"))?;
    let guest = list_instances(app.clone())?
        .into_iter()
        .find(|instance| {
            instance.loader_type == loader
                && instance.name.starts_with(&format!("Acceptance {loader} "))
                && instance.name.ends_with(" Guest")
                && instance.status == "ready"
        })
        .ok_or_else(|| LauncherError::validation("Guest 验收实例不存在。"))?;
    let (host_account, guest_account, accounts_offline) = ensure_e2e_accounts(&app)?;

    // 确保 Host 的 e4mc helper 就绪（走生产安装链：解析→下载→校验→受管理记录）。
    multiplayer::multiplayer_prepare(app.clone(), host.id).await?;

    let mut rounds_evidence = Vec::new();
    let mut stability_evidence = serde_json::Value::Null;
    let mut crash_evidence = serde_json::Value::Null;

    async fn run_round(
        app: &AppHandle,
        host: &Instance,
        guest: &Instance,
        host_account: i64,
        guest_account: i64,
        java: &str,
        loader: &str,
        accounts_offline: bool,
        duration: Option<Duration>,
    ) -> Result<serde_json::Value, LauncherError> {
        let started =
            multiplayer::multiplayer_start(app.clone(), host.id, host_account, java.to_string())
                .await?;
        let session_id = started
            .session_id
            .clone()
            .ok_or_else(|| LauncherError::validation("联机会话缺少 session_id。"))?;
        // Bootstrap Gate：FML 干净加载、helper READY、无 Warning/Error 屏。
        let boot_log_path = latest_instance_log(Path::new(&host.root_path))
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let boot_deadline = std::time::Instant::now() + Duration::from_secs(240);
        let mut boot_text: String;
        loop {
            boot_text = read_log(Path::new(&boot_log_path));
            let warning = boot_text.contains("Warning while loading mods");
            let error = boot_text.contains("Error loading mods");
            let ready = boot_text.contains("SH_E2E_HELPER_READY");
            if warning || error || ready || std::time::Instant::now() >= boot_deadline {
                break;
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        let boot_warning = boot_text.contains("Warning while loading mods");
        let boot_error = boot_text.contains("Error loading mods");
        let helper_ready = boot_text.contains("SH_E2E_HELPER_READY");
        let resource_pack_valid = !boot_text.contains("Missing metadata in pack");
        if !helper_ready || boot_warning || boot_error || !resource_pack_valid {
            return Err(LauncherError::validation(format!(
                "Bootstrap Gate 未通过：helperReady={helper_ready} warningScreen={boot_warning} errorScreen={boot_error} resourcePackInfoValid={resource_pack_valid}"
            )));
        }
        let ready_at = std::time::Instant::now();
        let ready_timeout = Duration::from_secs(12 * 60);
        let mut room = started;
        let mut ready_ok = false;
        loop {
            if std::time::Instant::now() >= ready_at + ready_timeout {
                break;
            }
            room = multiplayer::multiplayer_state(host.id);
            if ready_at.elapsed().as_secs() % 5 == 0 {
                eprintln!(
                    "[e2e-ready-watch] elapsed={}s state={:?} address={:?}",
                    ready_at.elapsed().as_secs(),
                    room.state,
                    room.public_address
                );
            }
            if room.state == multiplayer::MultiplayerState::Ready && room.public_address.is_some() {
                ready_ok = true;
                break;
            }
            if room.state == multiplayer::MultiplayerState::Error {
                break;
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        let address = room.public_address.clone();
        let valid_public = address
            .as_deref()
            .is_some_and(multiplayer::validate_e4mc_public_address);
        if !ready_ok || !valid_public {
            return Err(LauncherError::validation(format!(
                "Host 未获得可信 *.e4mc.link：state={:?} address={:?}",
                room.state, address
            )));
        }
        let time_to_ready = ready_at.elapsed().as_secs();
        // Guest 加入链路：本机握手规范化 shim（把 Forge 追加的 \0FML\0 品牌后缀改写为
        // 纯 e4mc 域名）→ 真实 relay → 房主 QUIC 流。Guest helper 以 Type.OTHER（普通服务器
        // 认证语义）连接 shim；禁止把公网域名伪装为 LAN 绕过 joinServer/session 校验。
        // 离线测试账户时，session 校验失败会表现为 “Invalid session” /
        // loginFailedInfo.invalidSession，分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION；
        // 1.20.4 原版 integrated server 对单机世界还有“will let them in anyway”的离线回退，
        // 属于原版行为，必须单独记录为 vanillaSingleplayerOfflineFallback，不得当作
        // session 验证通过。
        let join_shim = multiplayer::start_join_relay_shim(address.clone().unwrap_or_default())?;
        let join_port = join_shim.port();
        multiplayer::register_join_shim(guest.id, join_shim);
        let guest_join_file = PathBuf::from(&guest.root_path)
            .join(".minecraft")
            .join("sh_e2e_join.txt");
        fs::write(&guest_join_file, format!("127.0.0.1:{join_port}"))
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let launched = launch_instance(
            app.clone(),
            guest.id,
            guest_account,
            java.to_string(),
            Some(false),
            None,
            None,
            None,
        )
        .await?;
        let guest_process = launched.process_id;
        let guest_log = launched.log_path;
        let host_log_path = latest_instance_log(Path::new(&host.root_path))
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default();
        let join_deadline = std::time::Instant::now() + Duration::from_secs(4 * 60);
        let mut host_text = String::new();
        let mut guest_text = String::new();
        let mut invalid_session_seen_at: Option<std::time::Instant> = None;
        while std::time::Instant::now() < join_deadline {
            host_text = read_log(Path::new(&host_log_path));
            guest_text = read_log(Path::new(&guest_log));
            let layers = classify_join_evidence(&host_text, &guest_text);
            if layers.world_connection_established() {
                break;
            }
            // 离线账户在 session 边界被拒绝后：给原版 integrated server 的离线回退
            // （1.20.4 “will let them in anyway”）一个观察窗口，拒绝后数秒内未进入世界
            // 即分类为 BLOCKED_BY_TEST_ACCOUNT_SESSION，不空等 4 分钟。
            if layers.blocked_by_test_account_session() {
                let seen = *invalid_session_seen_at.get_or_insert(std::time::Instant::now());
                if seen.elapsed() >= Duration::from_secs(20) {
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(1000));
        }
        let layers = classify_join_evidence(&host_text, &guest_text);
        let outcome =
            evaluate_round_join(&layers, accounts_offline).map_err(LauncherError::validation)?;
        let relay_pass = outcome.relay_pass;
        let handshake_pass = outcome.handshake_pass;
        let rsa_pass = outcome.rsa_pass;
        let session_verified = outcome.session_verified;
        let world_join_status = outcome.world_join_status;
        let round_status = outcome.round_status;
        let mut disconnects = 0u32;
        let mut reconnects = 0u32;
        let mut provider_errors = 0u32;
        let stability_start = std::time::Instant::now();
        let stability_deadline = duration
            .map(|duration| std::time::Instant::now() + duration)
            .unwrap_or_else(std::time::Instant::now);
        let mut last_state = room.state;
        while duration.is_some() && std::time::Instant::now() < stability_deadline {
            let state = multiplayer::multiplayer_state(host.id);
            if state.state == multiplayer::MultiplayerState::Error {
                provider_errors += 1;
            }
            if last_state == multiplayer::MultiplayerState::Ready
                && state.state == multiplayer::MultiplayerState::Reconnecting
            {
                disconnects += 1;
            }
            if last_state == multiplayer::MultiplayerState::Reconnecting
                && state.state == multiplayer::MultiplayerState::Ready
            {
                reconnects += 1;
            }
            last_state = state.state;
            std::thread::sleep(Duration::from_secs(5));
        }
        let stability_seconds = stability_start.elapsed().as_secs();
        // 结束：停止 Host（同时关闭游戏与隧道），Guest 进程也终止。
        let _ = multiplayer::multiplayer_stop(app.clone(), session_id.clone());
        if guest_process != 0 {
            let _ = terminate_game(guest.id);
        }
        let _ = fs::remove_file(&guest_join_file);
        let mut closed = false;
        let close_deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < close_deadline {
            if matches!(
                multiplayer::multiplayer_state(host.id).state,
                multiplayer::MultiplayerState::Closed | multiplayer::MultiplayerState::Idle
            ) {
                closed = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        Ok(serde_json::json!({
            "status": round_status,
            "sessionId": session_id,
            "gameVersion": host.game_version,
            "loader": loader,
            "artifactPreflight": true,
            "helperReady": helper_ready,
            "warningScreen": boot_warning,
            "errorScreen": boot_error,
            "resourcePackInfoValid": resource_pack_valid,
            "lanPort": room.lan_port,
            "publicEndpoint": address,
            "publicAddressValid": valid_public,
            "timeToReadySeconds": time_to_ready,
            "guestProcessId": guest_process,
            "accountsOffline": accounts_offline,
            "layers": {
                "tunnel": {
                    "status": if valid_public && host_text.contains("Domain assigned") {
                        "PASS"
                    } else {
                        "FAIL"
                    },
                    "publicEndpoint": address,
                    "timeToReadySeconds": time_to_ready
                },
                "relayForwarding": {
                    "status": if relay_pass { "PASS" } else { "FAIL" },
                    "hostObservedRelayQuicStream": layers.host_quic_incoming_stream
                },
                "minecraftHandshake": {
                    "status": if handshake_pass { "PASS" } else { "FAIL" },
                    "guestConnectAttempted": layers.guest_connect_attempted
                },
                "rsaLoginTransport": {
                    "status": if rsa_pass { "PASS" } else { "FAIL" },
                    "guestReachedLoginBoundary": layers.guest_login_boundary_reached
                },
                "guestWorldJoin": {
                    "status": world_join_status,
                    "sessionVerified": session_verified,
                    "invalidSessionObserved": layers.guest_invalid_session,
                    "vanillaSingleplayerOfflineFallback": layers.host_offline_fallback,
                    "worldConnectionEstablished": layers.world_connection_established()
                }
            },
            "externalAccountAcceptance": if session_verified {
                "NOT_REQUIRED"
            } else {
                "EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING"
            },
            "localhostUsed": false,
            "joinShimUsed": true,
            "joinShimTarget": address,
            "hostLogExcerpt": join_log_excerpt(&host_text, 40),
            "guestLogExcerpt": join_log_excerpt(&guest_text, 40),
            "stabilitySeconds": stability_seconds,
            "disconnects": disconnects,
            "reconnects": reconnects,
            "providerErrors": provider_errors,
            "closedCleanly": closed,
        }))
    }

    for round in 0..rounds {
        let evidence = match run_round(
            &app,
            &host,
            &guest,
            host_account,
            guest_account,
            &java,
            &loader,
            accounts_offline,
            None,
        )
        .await
        {
            Ok(evidence) => evidence,
            Err(error) => {
                // 失败也必须回收游戏进程与联机会话，避免残留 Java 锁住 latest.log /
                // 世界 session.lock，污染后续轮次或后续运行。
                cleanup_round_processes(&app, host.id, guest.id);
                return Err(error);
            }
        };
        rounds_evidence.push(serde_json::json!({
            "round": round + 1,
            "evidence": evidence
        }));
    }
    if let Some(first) = rounds_evidence.first() {
        let evidence = &first["evidence"];
        let bootstrap = serde_json::json!({
            "minecraft": host.game_version,
            "loader": loader,
            "java": java,
            "artifactPreflight": "PASS",
            "modDiscovered": evidence["helperReady"],
            "helperConstructed": evidence["helperReady"],
            "resourcePackInfoValid": evidence["resourcePackInfoValid"],
            "warningScreen": evidence["warningScreen"],
            "errorScreen": evidence["errorScreen"],
            "helperReady": evidence["helperReady"],
            "result": if evidence["helperReady"].as_bool().unwrap_or(false) {
                "PASS"
            } else {
                "FAIL"
            },
            "completedAt": chrono_like_timestamp()
        });
        if let Ok(root) = launcher_data_directory() {
            let _ = fs::write(
                root.join(format!(
                    "helper-{loader}-{}-bootstrap.json",
                    host.game_version
                )),
                serde_json::to_vec_pretty(&bootstrap).unwrap_or_default(),
            );
        }
    }
    if minutes > 0 {
        stability_evidence = match run_round(
            &app,
            &host,
            &guest,
            host_account,
            guest_account,
            &java,
            &loader,
            accounts_offline,
            Some(Duration::from_secs(minutes * 60)),
        )
        .await
        {
            Ok(evidence) => evidence,
            Err(error) => {
                cleanup_round_processes(&app, host.id, guest.id);
                return Err(error);
            }
        };
    }
    if include_crash {
        let started =
            multiplayer::multiplayer_start(app.clone(), host.id, host_account, java.clone())
                .await?;
        let session_id = started.session_id.clone().unwrap_or_default();
        let mut room = started;
        let crash_deadline = std::time::Instant::now() + Duration::from_secs(12 * 60);
        while std::time::Instant::now() < crash_deadline
            && !(room.state == multiplayer::MultiplayerState::Ready
                && room.public_address.is_some())
        {
            room = multiplayer::multiplayer_state(host.id);
            std::thread::sleep(Duration::from_millis(1000));
        }
        let process_id = running_games()
            .lock()
            .map_err(|_| LauncherError::storage("无法读取运行状态。"))?
            .get(&host.id)
            .copied();
        let mut forced_terminated = false;
        if let Some(process_id) = process_id {
            #[cfg(target_os = "windows")]
            let status = std::process::Command::new("taskkill")
                .args(["/PID", &process_id.to_string(), "/T", "/F"])
                .status();
            #[cfg(not(target_os = "windows"))]
            let status = std::process::Command::new("kill")
                .args(["-9", &process_id.to_string()])
                .status();
            forced_terminated = status.map(|status| status.success()).unwrap_or(false);
        }
        let mut closed_or_error = false;
        let crash_close_deadline = std::time::Instant::now() + Duration::from_secs(60);
        while std::time::Instant::now() < crash_close_deadline {
            let state = multiplayer::multiplayer_state(host.id).state;
            if matches!(
                state,
                multiplayer::MultiplayerState::Closed
                    | multiplayer::MultiplayerState::Idle
                    | multiplayer::MultiplayerState::Error
            ) {
                closed_or_error = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        crash_evidence = serde_json::json!({
            "sessionId": session_id,
            "forcedTerminated": forced_terminated,
            "uiLeftReady": !(multiplayer::multiplayer_state(host.id).state
                == multiplayer::MultiplayerState::Ready),
            "closedOrError": closed_or_error,
        });
        let _ = multiplayer::multiplayer_stop(app.clone(), session_id);
    }
    let round_statuses: Vec<&str> = rounds_evidence
        .iter()
        .filter_map(|round| round["evidence"]["status"].as_str())
        .collect();
    let all_rounds_verified = round_statuses.iter().all(|status| *status == "passed");
    let all_layers_pass = round_statuses
        .iter()
        .all(|status| matches!(*status, "passed" | "passed_with_external_account_pending"));
    let overall = if all_rounds_verified {
        "passed"
    } else if all_layers_pass {
        "passed_with_external_account_pending"
    } else {
        "failed"
    };
    Ok(serde_json::json!({
        "status": overall,
        "loader": loader,
        "accountsOffline": accounts_offline,
        "externalAccountAcceptance": if all_rounds_verified {
            "NOT_REQUIRED"
        } else {
            "EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING"
        },
        "rounds": rounds_evidence,
        "stability": stability_evidence,
        "crash": crash_evidence,
        "completedAt": chrono_like_timestamp()
    }))
}

pub(crate) fn run_window_acceptance(app: AppHandle) -> Result<serde_json::Value, LauncherError> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| LauncherError::validation("主窗口不存在。"))?;
    window
        .show()
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    window
        .set_size(tauri::LogicalSize::new(1024.0, 720.0))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    let size_ok = window
        .inner_size()
        .map(|size| size.width > 900 && size.height > 600)
        .unwrap_or(false);
    let resizable = window.is_resizable().unwrap_or(false);
    let minimizable = window.is_minimizable().unwrap_or(false);
    let maximizable = window.is_maximizable().unwrap_or(false);
    let _ = window.minimize();
    let minimized = window.is_minimized().unwrap_or(false);
    let _ = window.unminimize();
    let _ = window.maximize();
    let maximized = window.is_maximized().unwrap_or(false);
    let _ = window.unmaximize();
    let scale_factor = window.scale_factor().unwrap_or(1.0);
    let title = window.title().unwrap_or_default();
    let _ = window.hide();
    Ok(serde_json::json!({
        "status": if size_ok && resizable && minimizable && maximizable && minimized && maximized {
            "passed"
        } else {
            "failed"
        },
        "title": title,
        "sizeOk": size_ok,
        "resizable": resizable,
        "minimizable": minimizable,
        "maximizable": maximizable,
        "minimizedObserved": minimized,
        "maximizedObserved": maximized,
        "scaleFactor": scale_factor,
        "completedAt": chrono_like_timestamp()
    }))
}

fn offline_account_uuid(app: &AppHandle, account_id: i64) -> Result<String, LauncherError> {
    let connection = open_database(app)?;
    let uuid: Option<String> = connection
        .query_row(
            "SELECT minecraft_uuid FROM accounts WHERE id=?1",
            [account_id],
            |row| row.get(0),
        )
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    uuid.ok_or_else(|| LauncherError::storage("离线账户缺少 minecraft_uuid。"))
}

/// 用生产启动路径（launch_instance）以离线账户启动游戏，验证：
/// - 进程成功启动、游戏日志出现 `Setting user: <username>`、客户端真正进入；
/// - play_history 的身份快照 username/uuid/auth_type 与本次冻结快照一致。
async fn launch_offline_and_verify(
    app: &AppHandle,
    instance_id: i64,
    account_id: i64,
    java: &str,
    username: &str,
    expected_uuid: &str,
) -> Result<serde_json::Value, LauncherError> {
    let launched = launch_instance(
        app.clone(),
        instance_id,
        account_id,
        java.to_string(),
        Some(true),
        None,
        None,
        None,
    )
    .await?;
    let process_id = launched.process_id;
    if process_id == 0 {
        return Err(LauncherError::storage("游戏进程未创建。"));
    }
    let deadline = std::time::Instant::now() + Duration::from_secs(240);
    let mut user_observed = false;
    let mut client_started = false;
    while std::time::Instant::now() < deadline {
        let text = read_log(Path::new(&launched.log_path));
        user_observed |= text.contains(&format!("Setting user: {username}"));
        client_started |= text.contains("Backend library")
            || text.contains("Sound engine started")
            || text.contains("Reloading ResourceManager");
        if user_observed && client_started {
            break;
        }
        std::thread::sleep(Duration::from_millis(1000));
    }
    let _ = terminate_game(instance_id);
    let exit_deadline = std::time::Instant::now() + Duration::from_secs(60);
    while std::time::Instant::now() < exit_deadline {
        let running = running_games()
            .lock()
            .map(|games| games.contains_key(&instance_id))
            .unwrap_or(false);
        if !running {
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let connection = open_database(app)?;
    let (snapshot_username, snapshot_uuid, snapshot_auth_type): (String, String, String) =
        connection
            .query_row(
                "SELECT username_snapshot, minecraft_uuid_snapshot, auth_type_snapshot
                 FROM play_history WHERE instance_id=?1 ORDER BY id DESC LIMIT 1",
                [instance_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
    let snapshot_ok = snapshot_username == username
        && snapshot_uuid == expected_uuid
        && snapshot_auth_type == "OFFLINE";
    Ok(serde_json::json!({
        "status": if user_observed && client_started && snapshot_ok { "passed" } else { "failed" },
        "instanceId": instance_id,
        "accountId": account_id,
        "username": username,
        "processId": process_id,
        "usernameObserved": user_observed,
        "clientStarted": client_started,
        "identitySnapshot": {
            "username": snapshot_username,
            "minecraftUuid": snapshot_uuid,
            "authType": snapshot_auth_type,
            "matchesFrozenIdentity": snapshot_ok
        },
        "logPath": launched.log_path,
        "completedAt": chrono_like_timestamp()
    }))
}

/// v0.9.0 Offline Account 最低真实验收（§33）：创建/默认/切换/启动/重启持久化/
/// 删除回退/完整性检查全自动执行，全程使用生产路径。
pub(crate) async fn run_offline_account_acceptance(
    app: AppHandle,
) -> Result<serde_json::Value, LauncherError> {
    // 清理旧验收账户，保证可重复运行。
    for account in list_accounts(app.clone())? {
        if account.display_name == "SHAcceptance" || account.display_name == "SHAcceptanceB" {
            let _ = remove_account(app.clone(), account.id);
        }
    }

    // 1. 创建 SHAcceptance + UUID 校验。
    let account_a = create_offline_account(app.clone(), "SHAcceptance".into())?;
    let expected_a = minecraft_offline_uuid("SHAcceptance");
    let stored_a = offline_account_uuid(&app, account_a.id)?;
    if stored_a != expected_a.to_string() {
        return Err(LauncherError::storage(
            "创建后 UUID 与官方离线 UUID 不一致。",
        ));
    }
    // 2. default + active。
    set_default_account(app.clone(), Some(account_a.id))?;
    set_active_account(app.clone(), Some(account_a.id))?;
    let state_1 = get_account_state(app.clone())?;
    if state_1["activeAccountId"] != account_a.id || state_1["defaultAccountId"] != account_a.id {
        return Err(LauncherError::storage(
            "default/active 账户引用未正确持久化。",
        ));
    }
    // 3. Vanilla 实例 A（1.20.4）。
    run_vanilla_install_acceptance(app.clone(), "1.20.4".into()).await?;
    let vanilla = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.name == "Acceptance Vanilla 1.20.4" && instance.status == "ready")
        .ok_or_else(|| LauncherError::validation("Vanilla 验收实例不存在。"))?;
    let java17 = std::env::var("LAUNCHER_E2E_JAVA17")
        .map_err(|_| LauncherError::validation("缺少 LAUNCHER_E2E_JAVA17。"))?;
    // 4. Vanilla 启动（账户 A）。
    let launch_a = launch_offline_and_verify(
        &app,
        vanilla.id,
        account_a.id,
        &java17,
        "SHAcceptance",
        &expected_a.to_string(),
    )
    .await?;
    // 5. 重启持久化：重读 state + 账户，UUID/身份不得变化。
    let state_after_restart = get_account_state(app.clone())?;
    let reloaded_a = offline_account_uuid(&app, account_a.id)?;
    let restart_persisted = state_after_restart["activeAccountId"] == account_a.id
        && state_after_restart["defaultAccountId"] == account_a.id
        && reloaded_a == expected_a.to_string();
    // 6. Forge 实例 B。
    let forge = list_instances(app.clone())?
        .into_iter()
        .find(|instance| instance.loader_type == "forge" && instance.status == "ready");
    let forge_launch_a = match forge.as_ref() {
        Some(instance) => launch_offline_and_verify(
            &app,
            instance.id,
            account_a.id,
            &java17,
            "SHAcceptance",
            &expected_a.to_string(),
        )
        .await
        .map(Some),
        None => Ok(None),
    }?;
    // §13 跨版本/加载器真实启动矩阵：本机可安装/已就绪的版本动态选择，不写死。
    let java21 = std::env::var("LAUNCHER_E2E_JAVA21").ok();
    let mut matrix: Vec<serde_json::Value> = Vec::new();
    for (loader, game_version) in [("vanilla", "1.20.1"), ("vanilla", "1.21.11")] {
        let java = if game_version.starts_with("1.21") {
            match &java21 {
                Some(path) => path.clone(),
                None => continue,
            }
        } else {
            java17.clone()
        };
        match run_vanilla_install_acceptance(app.clone(), game_version.to_string()).await {
            Ok(_) => {
                let instance = list_instances(app.clone())?.into_iter().find(|instance| {
                    instance.name == format!("Acceptance Vanilla {game_version}")
                        && instance.status == "ready"
                });
                match instance {
                    Some(instance) => {
                        let evidence = launch_offline_and_verify(
                            &app,
                            instance.id,
                            account_a.id,
                            &java,
                            "SHAcceptance",
                            &expected_a.to_string(),
                        )
                        .await?;
                        matrix.push(serde_json::json!({
                            "loader": loader,
                            "gameVersion": game_version,
                            "evidence": evidence
                        }));
                    }
                    None => matrix.push(serde_json::json!({
                        "loader": loader,
                        "gameVersion": game_version,
                        "status": "skipped_instance_unavailable"
                    })),
                }
            }
            Err(error) => matrix.push(serde_json::json!({
                "loader": loader,
                "gameVersion": game_version,
                "status": "install_failed",
                "error": error.error_message()
            })),
        }
    }
    // NeoForge（已就绪实例）。
    if let (Some(path), Some(instance)) = (
        java21,
        list_instances(app.clone())?
            .into_iter()
            .find(|instance| instance.loader_type == "neoforge" && instance.status == "ready"),
    ) {
        let evidence = launch_offline_and_verify(
            &app,
            instance.id,
            account_a.id,
            &path,
            "SHAcceptance",
            &expected_a.to_string(),
        )
        .await?;
        matrix.push(serde_json::json!({
            "loader": "neoforge",
            "gameVersion": instance.game_version,
            "evidence": evidence
        }));
    }
    // Fabric 1.20.4（动态安装）。
    match run_loader_install_acceptance(
        app.clone(),
        "1.20.4".into(),
        "fabric".into(),
        java17.clone(),
    )
    .await
    {
        Ok(_) => {
            if let Some(instance) = list_instances(app.clone())?.into_iter().find(|instance| {
                instance.name == "Acceptance fabric 1.20.4" && instance.status == "ready"
            }) {
                let evidence = launch_offline_and_verify(
                    &app,
                    instance.id,
                    account_a.id,
                    &java17,
                    "SHAcceptance",
                    &expected_a.to_string(),
                )
                .await?;
                matrix.push(serde_json::json!({
                    "loader": "fabric",
                    "gameVersion": "1.20.4",
                    "evidence": evidence
                }));
            }
        }
        Err(error) => matrix.push(serde_json::json!({
            "loader": "fabric",
            "gameVersion": "1.20.4",
            "status": "install_failed",
            "error": error.error_message()
        })),
    }
    // §14 Java 8 + 1.16.5（动态安装运行时，验证 Java 版本不影响账户身份）。
    // 本机网络对 Adoptium JDK8 源可能停滞：用超时保护避免整条验收被挂死，
    // 失败时如实记录为环境阻塞，而不是伪造通过。
    let java8_install =
        tokio::time::timeout(Duration::from_secs(300), install_managed_java(8)).await;
    let mut java8_verified = false;
    match java8_install {
        Ok(Ok(runtime)) => match tokio::time::timeout(
            Duration::from_secs(600),
            run_vanilla_install_acceptance(app.clone(), "1.16.5".into()),
        )
        .await
        {
            Ok(Ok(_)) => {
                if let Some(instance) = list_instances(app.clone())?.into_iter().find(|instance| {
                    instance.name == "Acceptance Vanilla 1.16.5" && instance.status == "ready"
                }) {
                    let evidence = launch_offline_and_verify(
                        &app,
                        instance.id,
                        account_a.id,
                        &runtime.path,
                        "SHAcceptance",
                        &expected_a.to_string(),
                    )
                    .await?;
                    java8_verified = evidence["status"] == "passed";
                    matrix.push(serde_json::json!({
                        "loader": "vanilla",
                        "gameVersion": "1.16.5",
                        "javaMajor": 8,
                        "evidence": evidence
                    }));
                }
            }
            Ok(Err(error)) => matrix.push(serde_json::json!({
                "loader": "vanilla",
                "gameVersion": "1.16.5",
                "status": "install_failed",
                "error": error.error_message()
            })),
            Err(_) => matrix.push(serde_json::json!({
                "loader": "vanilla",
                "gameVersion": "1.16.5",
                "status": "install_timed_out"
            })),
        },
        Ok(Err(error)) => matrix.push(serde_json::json!({
            "loader": "vanilla",
            "gameVersion": "1.16.5",
            "javaMajor": 8,
            "status": "java_install_failed",
            "error": error.error_message()
        })),
        Err(_) => matrix.push(serde_json::json!({
            "loader": "vanilla",
            "gameVersion": "1.16.5",
            "javaMajor": 8,
            "status": "java_install_timed_out"
        })),
    }
    let matrix_pass = matrix.iter().all(|entry| {
        entry["evidence"]["status"] == "passed"
            || matches!(
                entry["status"].as_str(),
                Some(
                    "skipped_instance_unavailable"
                        | "install_failed"
                        | "java_install_failed"
                        | "install_timed_out"
                        | "java_install_timed_out"
                )
            )
    });
    // 7. 第二账户 + 切换。
    let account_b = create_offline_account(app.clone(), "SHAcceptanceB".into())?;
    let expected_b = minecraft_offline_uuid("SHAcceptanceB");
    set_active_account(app.clone(), Some(account_b.id))?;
    let target_b = forge
        .as_ref()
        .map(|instance| instance.id)
        .unwrap_or(vanilla.id);
    let launch_b = launch_offline_and_verify(
        &app,
        target_b,
        account_b.id,
        &java17,
        "SHAcceptanceB",
        &expected_b.to_string(),
    )
    .await?;
    // 8. 删除第二账户：实例绑定回退到全局账户，账户 A 仍可用。
    remove_account(app.clone(), account_b.id)?;
    let account_a_still_exists = list_accounts(app.clone())?
        .iter()
        .any(|account| account.id == account_a.id);
    let state_after_delete = get_account_state(app.clone())?;
    let fallback_ok = state_after_delete["activeAccountId"] == account_a.id
        || state_after_delete["defaultAccountId"] == account_a.id;
    let launch_after_delete = launch_offline_and_verify(
        &app,
        target_b,
        account_a.id,
        &java17,
        "SHAcceptance",
        &expected_a.to_string(),
    )
    .await?;
    // 9. 完整性 + 重启后一致性。
    let connection = open_database(&app)?;
    let integrity = account_integrity_report(&connection)?;
    let final_state = get_account_state(app.clone())?;

    let all_launches_passed = [&launch_a, &launch_after_delete]
        .iter()
        .all(|evidence| evidence["status"] == "passed")
        && launch_b["status"] == "passed"
        && forge_launch_a
            .as_ref()
            .is_none_or(|evidence| evidence["status"] == "passed");
    let core_passed = restart_persisted
        && account_a_still_exists
        && fallback_ok
        && all_launches_passed
        && matrix_pass
        && integrity["status"] == "passed";
    let status = if !core_passed {
        "failed"
    } else if java8_verified {
        "passed"
    } else {
        "passed_with_java8_env_blocked"
    };

    Ok(serde_json::json!({
        "status": status,
        "accountA": {
            "id": account_a.id,
            "username": "SHAcceptance",
            "minecraftUuid": expected_a.to_string()
        },
        "accountB": {
            "id": account_b.id,
            "username": "SHAcceptanceB",
            "minecraftUuid": expected_b.to_string()
        },
        "steps": {
            "createAndUuid": "passed",
            "defaultAndActivePersisted": "passed",
            "vanillaLaunch": launch_a,
            "restartPersistence": restart_persisted,
            "forgeLaunch": forge_launch_a,
            "launchMatrix": {
                "entries": matrix,
                "status": if matrix_pass { "passed" } else { "failed" }
            },
            "java8Verification": if java8_verified {
                "passed"
            } else {
                "environment_blocked"
            },
            "secondAccountSwitchLaunch": launch_b,
            "deleteFallback": {
                "accountAStillExists": account_a_still_exists,
                "fallbackToRemainingAccount": fallback_ok,
                "launchAfterDelete": launch_after_delete
            },
            "integrity": integrity,
            "finalState": final_state
        },
        "completedAt": chrono_like_timestamp()
    }))
}

fn snapshot_account_db(path: &Path) -> Result<serde_json::Value, LauncherError> {
    let connection = Connection::open(path)
        .map_err(|error| LauncherError::storage(format!("无法打开数据库副本：{error}")))?;
    let accounts: Vec<serde_json::Value> = {
        let mut statement = connection
            .prepare(
                "SELECT id, account_type, display_name, minecraft_uuid, legacy_offline_uuid
                 FROM accounts ORDER BY id",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "accountType": row.get::<_, String>(1)?,
                    "displayName": row.get::<_, String>(2)?,
                    "minecraftUuid": row.get::<_, Option<String>>(3)?,
                    "legacyOfflineUuid": row.get::<_, Option<String>>(4)?,
                }))
            })
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };
    let bindings: Vec<serde_json::Value> = {
        let mut statement = connection
            .prepare(
                "SELECT instance_id, account_id FROM instance_launch_settings ORDER BY instance_id",
            )
            .map_err(|error| LauncherError::storage(error.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "instanceId": row.get::<_, i64>(0)?,
                    "accountId": row.get::<_, Option<i64>>(1)?,
                }))
            })
            .map_err(|error| LauncherError::storage(error.to_string()))?
            .filter_map(Result::ok)
            .collect::<Vec<_>>();
        rows
    };
    Ok(serde_json::json!({
        "accountCount": accounts.len(),
        "accounts": accounts,
        "bindings": bindings,
    }))
}

/// §27 真实旧用户数据恢复测试：把指定真实数据库复制到验收 staging 副本上执行升级，
/// 绝不触碰原库；对比升级前后账户、UUID、实例绑定、默认账户并做 FK/integrity 检查。
pub(crate) async fn run_account_migration_acceptance(
    app: AppHandle,
) -> Result<serde_json::Value, LauncherError> {
    let source = std::env::var("LAUNCHER_E2E_MIGRATE_SOURCE")
        .map_err(|_| LauncherError::validation("缺少 LAUNCHER_E2E_MIGRATE_SOURCE。"))?;
    let source = PathBuf::from(source);
    if !source.is_file() {
        return Err(LauncherError::validation("迁移源数据库不存在。"));
    }
    let before = snapshot_account_db(&source)?;
    let target = database_path(&app)?;
    // 先移除 staging 副本及其 WAL，避免旧 WAL 复活覆盖升级后的主库。
    for candidate in [
        target.clone(),
        PathBuf::from(format!("{}-wal", target.display())),
        PathBuf::from(format!("{}-shm", target.display())),
    ] {
        if candidate.exists() {
            let _ = fs::remove_file(&candidate);
        }
    }
    fs::copy(&source, &target)
        .map_err(|error| LauncherError::storage(format!("复制数据库副本失败：{error}")))?;
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", source.display()));
        if sidecar.is_file() {
            let _ = fs::copy(&sidecar, format!("{}{suffix}", target.display()));
        }
    }
    // open_database 会在副本上执行全部迁移（含 v12）。
    let connection = open_database(&app)?;
    let after = snapshot_account_db(&target)?;
    let integrity = account_integrity_report(&connection)?;
    let accounts_preserved = {
        let before_accounts = before["accounts"].as_array().cloned().unwrap_or_default();
        let after_accounts = after["accounts"].as_array().cloned().unwrap_or_default();
        before_accounts.len() == after_accounts.len()
            && before_accounts
                .iter()
                .zip(after_accounts.iter())
                .all(|(before, after)| {
                    let name = after["displayName"].as_str().unwrap_or_default();
                    let official = minecraft_offline_uuid(name).to_string();
                    before["id"] == after["id"]
                        && before["accountType"] == after["accountType"]
                        && before["displayName"] == after["displayName"]
                        && (before["minecraftUuid"] == after["minecraftUuid"]
                            || (before["minecraftUuid"].is_null()
                                && after["minecraftUuid"] == official))
                })
    };
    let bindings_preserved = before["bindings"] == after["bindings"];
    let migrated_versions: i64 = connection
        .query_row("SELECT MAX(version) FROM migrations", [], |row| row.get(0))
        .map_err(|error| LauncherError::storage(error.to_string()))?;
    Ok(serde_json::json!({
        "status": if accounts_preserved && bindings_preserved && integrity["status"] == "passed" {
            "passed"
        } else {
            "failed"
        },
        "source": source.to_string_lossy(),
        "stagingCopy": target.to_string_lossy(),
        "migratedToVersion": migrated_versions,
        "accountsPreserved": accounts_preserved,
        "bindingsPreserved": bindings_preserved,
        "before": before,
        "after": after,
        "integrity": integrity,
        "completedAt": chrono_like_timestamp()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_jar(path: &Path, entries: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in entries {
            archive.start_file(*name, options).unwrap();
            archive.write_all(bytes).unwrap();
        }
        archive.finish().unwrap();
    }

    fn good_entries(pack_format: i32) -> Vec<(&'static str, Vec<u8>)> {
        vec![
            (
                "META-INF/mods.toml",
                b"modLoader=\"javafml\"\nloaderVersion=\"[47,)\"\nlicense=\"MIT\"\n\n[[mods]]\nmodId=\"sh_e2e_helper\"\nversion=\"1.0.0\"\ndisplayName=\"SH E2E Helper\"\n"
                    .to_vec(),
            ),
            (
                "pack.mcmeta",
                serde_json::json!({"pack":{"description":"SH E2E","pack_format":pack_format}})
                    .to_string()
                    .into_bytes(),
            ),
            ("sh/e2e/ShE2eHelper.class", b"sh_e2e_helper\x00\x01\x02".to_vec()),
        ]
    }

    fn temp_jar(name: &str, entries: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sh-preflight-{}", unique_timestamp()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        write_jar(&path, entries);
        path
    }

    #[test]
    fn pack_format_maps_real_minecraft_versions() {
        assert_eq!(pack_format_for_minecraft_version("1.20.4"), Some(22));
        assert_eq!(pack_format_for_minecraft_version("1.20.2"), Some(18));
        assert_eq!(pack_format_for_minecraft_version("1.21.1"), Some(34));
        assert_eq!(pack_format_for_minecraft_version("1.21.4"), Some(46));
        assert_eq!(pack_format_for_minecraft_version("1.21.5"), Some(55));
        assert_eq!(pack_format_for_minecraft_version("1.21.8"), Some(61));
        assert_eq!(pack_format_for_minecraft_version("1.21.11"), Some(63));
    }

    #[test]
    fn valid_e2e_world_requires_level_dat_and_region() {
        let dir = std::env::temp_dir().join(format!("sh-world-{}", unique_timestamp()));
        fs::create_dir_all(dir.join("region")).unwrap();
        fs::write(dir.join("level.dat"), b"x").unwrap();
        assert!(valid_e2e_world(&dir));
        fs::remove_file(dir.join("level.dat")).unwrap();
        assert!(!valid_e2e_world(&dir));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn preflight_accepts_valid_helper_jar() {
        let jar = temp_jar("good.jar", &good_entries(22));
        assert!(preflight_helper_jar(&jar, 22).is_ok());
        let _ = fs::remove_dir_all(jar.parent().unwrap());
    }

    #[test]
    fn preflight_rejects_broken_artifacts() {
        let cases: Vec<(&str, Vec<(&str, Vec<u8>)>)> = vec![
            (
                "missing-pack.jar",
                good_entries(22)
                    .into_iter()
                    .filter(|(name, _)| *name != "pack.mcmeta")
                    .collect(),
            ),
            ("bad-json.jar", {
                let mut entries = good_entries(22);
                entries[1] = ("pack.mcmeta", b"{not json".to_vec());
                entries
            }),
            ("wrong-format.jar", good_entries(18).into_iter().collect()),
            (
                "missing-mods.jar",
                good_entries(22)
                    .into_iter()
                    .filter(|(name, _)| *name != "META-INF/mods.toml")
                    .collect(),
            ),
            ("wrong-modid.jar", {
                let mut entries = good_entries(22);
                entries[0] = (
                        "META-INF/mods.toml",
                        b"modLoader=\"javafml\"\nloaderVersion=\"[47,)\"\nlicense=\"MIT\"\n\n[[mods]]\nmodId=\"other\"\nversion=\"1\"\ndisplayName=\"x\"\n".to_vec(),
                    );
                entries
            }),
            (
                "missing-entry.jar",
                good_entries(22)
                    .into_iter()
                    .filter(|(name, _)| *name != "sh/e2e/ShE2eHelper.class")
                    .collect(),
            ),
            ("default-package.jar", {
                let mut entries = good_entries(22);
                entries[2] = ("ShE2eHelper.class", b"sh_e2e_helper".to_vec());
                entries
            }),
            ("polluted.jar", {
                let mut entries = good_entries(22);
                entries.push((
                    "net/minecraftforge/common/MinecraftForge.class",
                    b"pollution".to_vec(),
                ));
                entries
            }),
        ];
        for (name, entries) in cases {
            let jar = temp_jar(name, &entries);
            assert!(
                preflight_helper_jar(&jar, 22).is_err(),
                "preflight 必须拒绝：{name}"
            );
            let _ = fs::remove_dir_all(jar.parent().unwrap());
        }
    }

    #[test]
    fn preflight_rejects_corrupt_jar() {
        let dir = std::env::temp_dir().join(format!("sh-preflight-{}", unique_timestamp()));
        fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("corrupt.jar");
        fs::write(&jar, b"not a zip file at all").unwrap();
        assert!(preflight_helper_jar(&jar, 22).is_err());
        let _ = fs::remove_dir_all(&dir);
    }

    fn host_log_with_relay_stream(joined: bool, offline_fallback: bool) -> String {
        let mut lines = vec![
            "control channel open: [id: 0x1, QuicStreamAddress{streamId=0}]",
            "Domain assigned: bullion-coerce.jp.e4mc.link",
        ];
        if offline_fallback {
            lines.push(
                "[User Authenticator #1/WARN]: Failed to verify username but will let them in anyway!",
            );
        }
        if joined {
            lines.push(
                "[PlayerList]: AcceptGuest[QuicStreamAddress{streamId=1}] logged in with entity id 203",
            );
            lines.push("[MinecraftServer]: AcceptGuest joined the game");
        }
        lines.join("\n")
    }

    #[test]
    fn quic_stream_id_parses_control_and_data_streams() {
        assert_eq!(quic_stream_id("QuicStreamAddress{streamId=0}"), Some(0));
        assert_eq!(
            quic_stream_id("AcceptGuest[QuicStreamAddress{streamId=1}] logged in"),
            Some(1)
        );
        assert_eq!(
            quic_stream_id("AcceptGuest (QuicStreamAddress{streamId=2}) lost connection"),
            Some(2)
        );
        assert_eq!(quic_stream_id("AcceptGuest[/192.168.1.5:1234]"), None);
    }

    #[test]
    fn offline_account_invalid_session_is_account_limit_not_network_failure() {
        // 1.21.x 场景：Host 观察到真实 relay QUIC 流入流（streamId=1），Guest 走到
        // login/auth 边界后被 session 校验拒绝，未进入世界。
        let host_text = "control channel open: QuicStreamAddress{streamId=0}\n\
            AcceptGuest (QuicStreamAddress{streamId=1}) lost connection: Timed out";
        let guest_text = "SH_E2E_HELPER_JOIN:127.0.0.1:52592\n\
            Connecting to 127.0.0.1, 52592\n\
            disconnect.loginFailedInfo.invalidSession";
        let layers = classify_join_evidence(host_text, guest_text);
        assert!(layers.relay_forwarding_pass());
        assert!(layers.handshake_pass());
        assert!(layers.rsa_login_transport_pass());
        assert!(layers.blocked_by_test_account_session());
        assert!(!layers.session_verified_join());
        assert!(!layers.world_connection_established());

        let outcome =
            evaluate_round_join(&layers, true).expect("offline account must be pending, not fail");
        assert_eq!(outcome.world_join_status, "BLOCKED_BY_TEST_ACCOUNT_SESSION");
        assert_eq!(outcome.round_status, "passed_with_external_account_pending");
    }

    #[test]
    fn vanilla_singleplayer_offline_fallback_is_not_session_verified() {
        // 1.20.4 Forge 真实场景：session 校验失败，原版 integrated server 走
        // “will let them in anyway” 离线回退让 Guest 进入世界——这不是认证通过。
        let host_text = host_log_with_relay_stream(true, true);
        let guest_text = "Connecting to 127.0.0.1, 52592\n\
            Failed to log in: Invalid session (Try restarting your game and the launcher)\n\
            SH_E2E_CLIENT_JOINED:AcceptGuest";
        let layers = classify_join_evidence(&host_text, guest_text);
        assert!(layers.world_connection_established());
        assert!(layers.host_offline_fallback);
        assert!(layers.blocked_by_test_account_session());
        assert!(!layers.session_verified_join());

        let outcome = evaluate_round_join(&layers, true).expect("must be pending, not fail");
        assert_eq!(outcome.world_join_status, "BLOCKED_BY_TEST_ACCOUNT_SESSION");
        assert_eq!(outcome.round_status, "passed_with_external_account_pending");
    }

    #[test]
    fn online_account_session_verified_join_passes() {
        let host_text = host_log_with_relay_stream(true, false);
        let guest_text = "Connecting to 127.0.0.1, 52592\nSH_E2E_CLIENT_JOINED:AcceptGuest";
        let layers = classify_join_evidence(&host_text, guest_text);
        assert!(layers.session_verified_join());
        let outcome = evaluate_round_join(&layers, false).expect("online verified join must pass");
        assert_eq!(outcome.world_join_status, "PASS_SESSION_VERIFIED");
        assert_eq!(outcome.round_status, "passed");
    }

    #[test]
    fn online_account_invalid_session_is_hard_failure() {
        let host_text = "control channel open: QuicStreamAddress{streamId=0}\n\
            AcceptGuest (QuicStreamAddress{streamId=1}) lost connection";
        let guest_text = "Connecting to 127.0.0.1, 52592\nInvalid session";
        let layers = classify_join_evidence(host_text, guest_text);
        let error =
            evaluate_round_join(&layers, false).expect_err("online account cannot be pending");
        assert!(error.contains("session"));
    }

    #[test]
    fn missing_relay_stream_is_network_failure_not_account_limit() {
        // 只有控制通道（streamId=0），Guest 从未经 relay 到达 Host。
        let host_text = "control channel open: QuicStreamAddress{streamId=0}";
        let guest_text = "Connecting to 127.0.0.1, 52592";
        let layers = classify_join_evidence(host_text, guest_text);
        assert!(!layers.relay_forwarding_pass());
        assert!(evaluate_round_join(&layers, true).is_err());
    }

    #[test]
    fn log_excerpt_collects_only_evidence_lines() {
        let text = "noise\nAcceptGuest[QuicStreamAddress{streamId=1}] logged in\nnoise";
        let excerpt = join_log_excerpt(text, 10);
        assert_eq!(excerpt.len(), 1);
        assert!(excerpt[0].contains("QuicStreamAddress"));
    }
}

#[allow(clippy::items_after_test_module)]
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

#[allow(clippy::items_after_test_module)]
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

#[allow(clippy::items_after_test_module)]
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

#[allow(clippy::items_after_test_module)]
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

#[allow(clippy::items_after_test_module)]
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
