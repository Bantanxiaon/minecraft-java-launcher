// MULTIPLAYER_ISOLATION_GATE:
// 1) 读取 docs/evidence/nextgen/multiplayer-isolation.json，任何隔离违规即 FAIL。
// 2) 静态审计生产前端：不得有服务器/联机导航、路由、CTA、e4mc 入口；
//    App.tsx 不得引用 ServersPage；生产 src 不得 invoke multiplayer_*。
// 3) 静态审计 Rust：所有 multiplayer 命令必须有 capability guard；
//    launch_instance 函数体不得调用 multiplayer_prepare/start/join/ensure/install。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[multiplayer-isolation-gate] FAIL: ${message}`);
  process.exit(1);
};

const evidencePath = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "multiplayer-isolation.json",
);
if (!fs.existsSync(evidencePath)) fail("缺少 multiplayer-isolation.json");
const evidence = JSON.parse(fs.readFileSync(evidencePath, "utf8"));

const violations = [];
if (evidence.productionUiVisible !== false)
  violations.push("productionUiVisible != false");
if (evidence.normalLaunchCanReachMultiplayer !== false)
  violations.push("normalLaunchCanReachMultiplayer != false");
if (evidence.normalLaunchMultiplayerNetworkRequests !== 0)
  violations.push("normalLaunchMultiplayerNetworkRequests != 0");
if (evidence.normalLaunchMultiplayerWrites !== 0)
  violations.push("normalLaunchMultiplayerWrites != 0");
if (evidence.autoE4mcInstallDuringNormalLaunch !== false)
  violations.push("autoE4mcInstallDuringNormalLaunch != false");

// 2. 生产前端静态审计
const appTsx = fs.readFileSync(path.join(REPO, "src", "App.tsx"), "utf8");
if (appTsx.includes("ServersPage")) violations.push("App.tsx 仍引用 ServersPage");
if (/invoke\(\s*["']multiplayer_/.test(appTsx)) {
  violations.push("App.tsx 存在 multiplayer_ invoke");
}
const productionDirs = [
  "src/app",
  "src/features",
  "src/components/OnboardingGuide.tsx",
  "src/components/TutorialModal.tsx",
  "src/components/SplashScreen.tsx",
  "src/ui",
];
const forbiddenPatterns = [
  /创建远程房间/,
  /快速加入/,
  /e4mc/,
  /联机记录/,
  /服务器管理中心/,
  /"联机"/,
  /'联机'/,
];
for (const dir of productionDirs) {
  const full = path.join(REPO, dir);
  if (!fs.existsSync(full)) continue;
  const files = [];
  const stat = fs.statSync(full);
  if (stat.isFile()) {
    files.push(full);
  } else {
    const walk = (d) => {
      for (const entry of fs.readdirSync(d, { withFileTypes: true })) {
        if (entry.name.startsWith(".")) continue;
        const target = path.join(d, entry.name);
        if (entry.isDirectory()) walk(target);
        else if (/\.(tsx|ts|css)$/.test(entry.name)) files.push(target);
      }
    };
    walk(full);
  }
  for (const file of files) {
    const content = fs.readFileSync(file, "utf8");
    for (const pattern of forbiddenPatterns) {
      if (pattern.test(content)) {
        violations.push(
          `生产文件 ${path.relative(REPO, file)} 含联机入口文案 ${pattern}`,
        );
      }
    }
  }
}

// 3. Rust 静态审计
const rustRoot = path.join(REPO, "src-tauri", "src");
const rustFiles = fs
  .readdirSync(rustRoot, { recursive: true, withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
  .map((entry) => ({
    name: entry.name,
    content: fs.readFileSync(
      path.join(entry.parentPath ?? rustRoot, entry.name),
      "utf8",
    ),
  }));
const multiplayerRs = rustFiles.find((file) => file.name === "multiplayer.rs");
if (!multiplayerRs) violations.push("缺少 multiplayer.rs（Multiplayer Core 被删除）");
const guardedCommands = [
  "multiplayer_prepare",
  "multiplayer_start",
  "multiplayer_stop",
  "multiplayer_cancel",
  "multiplayer_join",
  "multiplayer_state",
  "multiplayer_diagnostics",
  "multiplayer_history",
];
for (const command of guardedCommands) {
  const commandIndex = multiplayerRs.content.indexOf(`fn ${command}`);
  const guardIndex = multiplayerRs.content.indexOf("multiplayer_experimental_enabled");
  if (commandIndex === -1 || guardIndex === -1 || guardIndex > commandIndex) {
    violations.push(`${command} 缺少 capability guard`);
  }
}

const libRs = rustFiles.find((file) => file.name === "lib.rs");
const launchStart = libRs.content.indexOf("async fn launch_instance");
const launchEnd = libRs.content.indexOf("async fn fetch_version_details", launchStart);
if (launchStart === -1) {
  violations.push("未找到 launch_instance");
} else {
  const launchBody = libRs.content.slice(
    launchStart,
    launchEnd > 0 ? launchEnd : launchStart + 40000,
  );
  for (const forbidden of [
    "multiplayer_prepare(",
    "multiplayer_start(",
    "multiplayer_join(",
    "ensure_e4mc",
    "install_e4mc",
  ]) {
    if (launchBody.includes(forbidden)) {
      violations.push(`launch_instance 调用 ${forbidden}`);
    }
  }
}

const result = {
  ...evidence,
  staticAuditViolations: violations,
  gate: violations.length === 0 ? "PASS" : "FAIL",
  generatedAt: new Date().toISOString(),
};
fs.mkdirSync(path.join(REPO, "docs", "evidence", "nextgen"), { recursive: true });
fs.writeFileSync(
  path.join(REPO, "docs", "evidence", "nextgen", "multiplayer-isolation-gate.json"),
  JSON.stringify(result, null, 2) + "\n",
);

if (violations.length) {
  for (const violation of violations) console.error(`  - ${violation}`);
  console.error(`[multiplayer-isolation-gate] FAIL (${violations.length} violations)`);
  process.exit(1);
}
console.log("[multiplayer-isolation-gate] PASS");
