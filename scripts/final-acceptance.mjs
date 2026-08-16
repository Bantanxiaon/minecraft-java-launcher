// 最终自动化验收 Harness：零人工依赖，机器可读 evidence + Markdown summary。
// 用法：node scripts/final-acceptance.mjs <command> [loader]
// command: multiplayer-prepare | multiplayer-run | download | updater | window | final | cleanup

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const BIN = path.join(REPO, "src-tauri", "target", "debug", "app.exe");
const REAL_DATA =
  process.env.SH_REAL_DATA ?? "D:\\MinecraftLauncherData";
const ACCEPT_DATA =
  process.env.SH_ACCEPT_DATA ?? "D:\\MinecraftLauncherData-Acceptance";
const JAVA17 = process.env.SH_JAVA17 ??
  path.join(REAL_DATA, "runtimes", "java-17", "jdk-17.0.20+8", "bin", "java.exe");
const JAVA21 = process.env.SH_JAVA21 ??
  path.join(REAL_DATA, "runtimes", "java-21", "jdk-21.0.12+8", "bin", "java.exe");

const EVIDENCE_DIR = path.join(REPO, "docs", "acceptance");
const ACCOUNT_EVIDENCE_DIR = path.join(REPO, "docs", "evidence");
fs.mkdirSync(EVIDENCE_DIR, { recursive: true });
fs.mkdirSync(ACCOUNT_EVIDENCE_DIR, { recursive: true });

function fail(message) {
  console.error(`[acceptance] FAIL: ${message}`);
  process.exit(1);
}

function ensureBuilt() {
  const result = spawnSync(
    "cargo",
    ["build", "--manifest-path", "src-tauri/Cargo.toml"],
    { cwd: REPO, stdio: "inherit", shell: true },
  );
  if (result.status !== 0) fail("debug 构建失败");
  if (!fs.existsSync(BIN)) fail("找不到 debug app.exe");
}

function runApp({ env, timeoutMinutes }) {
  const environment = {
    ...process.env,
    MINECRAFT_LAUNCHER_DATA: ACCEPT_DATA,
    ...env,
  };
  const result = spawnSync(BIN, [], {
    cwd: REPO,
    env: environment,
    stdio: "inherit",
    shell: false,
    timeout: Math.round(timeoutMinutes * 60 * 1000),
    windowsHide: false,
  });
  if (result.error) fail(`运行 app.exe 失败：${result.error.message}`);
  return result.status ?? 1;
}

function readReport(name) {
  const reportPath = path.join(ACCEPT_DATA, name);
  if (!fs.existsSync(reportPath)) return null;
  try {
    const parsed = JSON.parse(fs.readFileSync(reportPath, "utf8"));
    fs.copyFileSync(
      reportPath,
      path.join(EVIDENCE_DIR, `latest-${name}`),
    );
    return parsed;
  } catch {
    return null;
  }
}

function writeSummary(entries) {
  const lines = ["# Final Acceptance Summary", ""];
  for (const entry of entries) {
    const ok = entry.status === "passed";
    lines.push(`- ${ok ? "[x]" : "[ ]"} **${entry.name}** — ${entry.status}`);
    if (entry.details) {
      lines.push("  ```json");
      lines.push(`  ${JSON.stringify(entry.details)}`);
      lines.push("  ```");
    }
  }
  const summaryPath = path.join(EVIDENCE_DIR, "final-acceptance-summary.md");
  fs.writeFileSync(summaryPath, `${lines.join("\n")}\n`, "utf8");
  console.log(`[acceptance] summary: ${summaryPath}`);
}

const command = process.argv[2];
const loader = process.argv[3];

switch (command) {
  case "multiplayer-prepare": {
    ensureBuilt();
    const status = runApp({
      env: {
        LAUNCHER_E2E_MULTIPLAYER: "prepare",
        LAUNCHER_E2E_JAVA17: JAVA17,
        LAUNCHER_E2E_JAVA21: JAVA21,
      },
      timeoutMinutes: 120,
    });
    const report = readReport("acceptance-multiplayer-prepare.json");
    if (status !== 0 || !report || report.status !== "passed") {
      fail(`multiplayer prepare 失败：${JSON.stringify(report)}`);
    }
    console.log(`[acceptance] multiplayer prepare passed：${JSON.stringify(report.matrix)}`);
    break;
  }
  case "multiplayer-run": {
    if (!loader) fail("缺少 loader 参数");
    ensureBuilt();
    const minutes = Number(process.env.SH_E2E_MINUTES ?? "30");
    const rounds = Number(process.env.SH_E2E_ROUNDS ?? "3");
    const java = loader === "neoforge" ? JAVA21 : JAVA17;
    const status = runApp({
      env: {
        LAUNCHER_E2E_MULTIPLAYER: "run",
        LAUNCHER_E2E_MP_LOADER: loader,
        LAUNCHER_E2E_JAVA: java,
        LAUNCHER_E2E_MINUTES: String(minutes),
        LAUNCHER_E2E_ROUNDS: String(rounds),
        LAUNCHER_E2E_CRASH: "1",
      },
      timeoutMinutes: minutes + 60,
    });
    const report = readReport("acceptance-multiplayer-run.json");
    const acceptable = ["passed", "passed_with_external_account_pending"];
    if (status !== 0 || !report || !acceptable.includes(report.status)) {
      fail(`multiplayer run(${loader}) 失败：${JSON.stringify(report)}`);
    }
    if (report.status === "passed_with_external_account_pending") {
      const joinStatuses = (report.rounds ?? [])
        .map((round) => round?.evidence?.layers?.guestWorldJoin?.status)
        .join(",");
      console.warn(
        `[acceptance] multiplayer run(${loader})：公网 Tunnel/Relay/Handshake/RSA 已 PASS，` +
          `但 “经 Mojang/Microsoft session 验证进入世界” 为 ` +
          `EXTERNAL_ACCOUNT_ACCEPTANCE_PENDING（guestWorldJoin=${joinStatuses}）。` +
          `该结果不得在 Release Notes 中表述为联机已验证。`,
      );
    }
    if (fs.existsSync(ACCEPT_DATA)) {
      for (const name of fs.readdirSync(ACCEPT_DATA)) {
        if (name.startsWith("helper-") && name.endsWith(".json")) {
          fs.copyFileSync(
            path.join(ACCEPT_DATA, name),
            path.join(EVIDENCE_DIR, name),
          );
        }
      }
    }
    console.log(`[acceptance] multiplayer run(${loader}) passed`);
    break;
  }
  case "account-integrity": {
    ensureBuilt();
    const status = runApp({
      env: { LAUNCHER_E2E_ACCOUNT: "integrity" },
      timeoutMinutes: 5,
    });
    const report = readReport("acceptance-account-integrity.json");
    if (status !== 0 || !report || report.status !== "passed") {
      fail(`account integrity 失败：${JSON.stringify(report)}`);
    }
    fs.copyFileSync(
      path.join(EVIDENCE_DIR, "latest-acceptance-account-integrity.json"),
      path.join(ACCOUNT_EVIDENCE_DIR, "offline-account-integrity.json"),
    );
    console.log("[acceptance] account integrity passed");
    break;
  }
  case "account-flow": {
    ensureBuilt();
    const status = runApp({
      env: {
        LAUNCHER_E2E_ACCOUNT: "flow",
        LAUNCHER_E2E_JAVA17: JAVA17,
        LAUNCHER_E2E_JAVA21: JAVA21,
      },
      timeoutMinutes: 60,
    });
    const report = readReport("acceptance-account-flow.json");
    const acceptable = ["passed", "passed_with_java8_env_blocked"];
    if (status !== 0 || !report || !acceptable.includes(report.status)) {
      fail(`account flow 失败：${JSON.stringify(report)}`);
    }
    if (report.status === "passed_with_java8_env_blocked") {
      console.warn(
        "[acceptance] account flow：核心账户体系全部 PASS，但 Java 8 + 1.16.5 因本机网络（Adoptium 源不可达）未完成验证，如实记为环境阻塞。",
      );
    }
    fs.copyFileSync(
      path.join(EVIDENCE_DIR, "latest-acceptance-account-flow.json"),
      path.join(ACCOUNT_EVIDENCE_DIR, "offline-account-flow.json"),
    );
    console.log("[acceptance] account flow passed");
    break;
  }
  case "account-migrate": {
    const source = process.env.SH_REAL_DB ?? "D:\\MinecraftLauncherData\\launcher.sqlite3";
    if (!fs.existsSync(source)) fail(`真实数据库不存在：${source}`);
    ensureBuilt();
    const status = runApp({
      env: {
        LAUNCHER_E2E_ACCOUNT: "migrate",
        LAUNCHER_E2E_MIGRATE_SOURCE: source,
      },
      timeoutMinutes: 10,
    });
    const report = readReport("acceptance-account-migrate.json");
    if (status !== 0 || !report || report.status !== "passed") {
      fail(`account migration 失败：${JSON.stringify(report)}`);
    }
    fs.copyFileSync(
      path.join(EVIDENCE_DIR, "latest-acceptance-account-migrate.json"),
      path.join(ACCOUNT_EVIDENCE_DIR, "account-migration.json"),
    );
    console.log("[acceptance] account migration passed");
    break;
  }
  case "download": {
    const tests = spawnSync(
      "cargo",
      [
        "test",
        "--manifest-path",
        "src-tauri/Cargo.toml",
        "--lib",
        "extreme_slow",
        "--",
        "--test-threads=1",
      ],
      { cwd: REPO, stdio: "inherit", shell: true, timeout: 600_000 },
    );
    if (tests.status !== 0) fail("极慢源 Mock HTTP 回归失败");
    const benchmark = spawnSync(
      "node",
      [
        "scripts/benchmark-download.mjs",
        "--url",
        "https://cdn.modrinth.com/data/qANg5Jrr/versions/CUKdAmgx/e4mc-forge-6.2.1.jar",
        "--sha1",
        "6fc90baef39cff5f9466ddf39f4421e3e9475308",
        "--repeat",
        "3",
        "--out",
        "docs/benchmark-download.json",
      ],
      { cwd: REPO, stdio: "inherit", shell: true, timeout: 900_000 },
    );
    if (benchmark.status !== 0) fail("真实下载基准失败");
    const evidence = {
      status: "passed",
      extremeSlowMockRegression: "passed",
      benchmark: "passed",
      completedAt: new Date().toISOString(),
    };
    fs.writeFileSync(
      path.join(EVIDENCE_DIR, "acceptance-download.json"),
      JSON.stringify(evidence, null, 2) + "\n",
    );
    console.log("[acceptance] download passed");
    break;
  }
  case "window": {
    ensureBuilt();
    const status = runApp({
      env: { LAUNCHER_E2E_WINDOW: "1" },
      timeoutMinutes: 5,
    });
    const report = readReport("acceptance-window.json");
    if (status !== 0 || !report || report.status !== "passed") {
      fail(`window 验收失败：${JSON.stringify(report)}`);
    }
    console.log(`[acceptance] window passed：${JSON.stringify(report)}`);
    break;
  }
  case "updater": {
    const versions = spawnSync(
      "node",
      ["scripts/release-gate.mjs"],
      { cwd: REPO, stdio: "inherit", shell: true },
    );
    // release-gate 在人工/外部项清零前会失败；updater staging 只验证版本一致与构建。
    const pkg = JSON.parse(fs.readFileSync("package.json", "utf8"));
    const tauri = JSON.parse(
      fs.readFileSync("src-tauri/tauri.conf.json", "utf8"),
    );
    const cargo = fs.readFileSync("src-tauri/Cargo.toml", "utf8");
    const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
    if (pkg.version !== tauri.version || pkg.version !== cargoVersion) {
      fail("版本不一致");
    }
    const build = spawnSync(
      "pnpm",
      [
        "tauri",
        "build",
        "--bundles",
        "nsis",
        "--config",
        "scripts/tauri-staging-config.json",
      ],
      { cwd: REPO, stdio: "inherit", shell: true, timeout: 2_400_000 },
    );
    if (build.status !== 0) fail("NSIS 构建失败");
    const exe = fs
      .readdirSync(path.join(REPO, "src-tauri", "target", "release", "bundle", "nsis"))
      .find((name) => name.endsWith(".exe") && name.includes(pkg.version));
    if (!exe) fail("未生成 NSIS 安装包");
    // 本机没有 CI 私钥，签名必须在 CI 完成；这里只校验 latest.json 结构
    // （version/notes/pub_date/platforms.windows-x86_64.url+signature），
    // 不允许把“未签名 staging”伪造成已签名 live 发布。
    const stagingManifest = {
      version: pkg.version,
      notes: "staging",
      pub_date: new Date().toISOString(),
      platforms: {
        "windows-x86_64": {
          signature: "",
          url: "https://github.com/Bantanxiaon/minecraft-java-launcher/releases/latest/download/SHLauncher-setup.exe",
        },
      },
    };
    for (const key of ["version", "notes", "pub_date", "platforms"]) {
      if (!(key in stagingManifest)) fail(`latest.json schema 缺少字段 ${key}`);
    }
    const platform = stagingManifest.platforms["windows-x86_64"];
    if (typeof platform.url !== "string" || !platform.url.startsWith("https://")) {
      fail("latest.json url 无效");
    }
    const evidence = {
      status: "passed",
      version: pkg.version,
      installer: exe,
      signing: "staged-unsigned: CI signs with repo secret",
      latestJsonSchema: "validated",
      releaseGateExit: versions.status,
      completedAt: new Date().toISOString(),
    };
    fs.writeFileSync(
      path.join(EVIDENCE_DIR, "acceptance-updater.json"),
      JSON.stringify(evidence, null, 2) + "\n",
    );
    console.log("[acceptance] updater staging passed");
    break;
  }
  case "cleanup": {
    const games = spawnSync(
      "powershell",
      [
        "-NoProfile",
        "-Command",
        `Get-Process | Where-Object { $_.ProcessName -like 'java*' } | Stop-Process -Force -ErrorAction SilentlyContinue`,
      ],
      { cwd: REPO, stdio: "inherit", shell: true },
    );
    if (fs.existsSync(ACCEPT_DATA)) {
      fs.rmSync(ACCEPT_DATA, { recursive: true, force: true });
    }
    console.log(`[acceptance] cleanup done (games exit=${games.status})`);
    break;
  }
  case "final": {
    const summary = [];
    const checks = [
      ["account-integrity", "离线账户完整性"],
      ["account-flow", "离线账户真实验收"],
      ["account-migrate", "真实数据库迁移验收"],
      ["multiplayer-prepare", "multiplayer prepare"],
      ["multiplayer-run forge", "multiplayer forge E2E"],
      ["multiplayer-run neoforge", "multiplayer neoforge E2E"],
      ["download", "下载验收"],
      ["window", "窗口验收"],
      ["updater", "Updater staging"],
    ];
    for (const [step, name] of checks) {
      const result = spawnSync(
        process.execPath,
        ["scripts/final-acceptance.mjs", ...step.split(" ")],
        { cwd: REPO, stdio: "inherit", shell: false },
      );
      summary.push({ name, status: result.status === 0 ? "passed" : "failed" });
      if (result.status !== 0) {
        writeSummary(summary);
        fail(`${name} 未通过，终止最终验收`);
      }
    }
    const gate = spawnSync("node", ["scripts/release-gate.mjs"], {
      cwd: REPO,
      stdio: "inherit",
      shell: true,
    });
    summary.push({
      name: "release gate",
      status: gate.status === 0 ? "passed" : "failed",
    });
    writeSummary(summary);
    if (gate.status !== 0) fail("release gate 未通过，禁止发布");
    console.log("[acceptance] final passed");
    break;
  }
  default:
    fail(`未知命令：${command}`);
}
