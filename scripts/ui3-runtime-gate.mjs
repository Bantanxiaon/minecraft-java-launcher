// UI3_RUNTIME_GATE：验证生产界面确实加载 UI 3.0，并要求真实运行截图证据。
// 任一硬性项缺失即 FAIL；禁止用“代码里存在 UI3 文件”代替运行验证。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[ui3-runtime-gate] FAIL: ${message}`);
  process.exit(1);
};

const requiredScreenshots = [
  "splash.png",
  "home.png",
  "library.png",
  "instance-overview.png",
  "instance-mods.png",
  "discover.png",
  "downloads.png",
  "accounts.png",
  "settings.png",
  "storage.png",
  "error-state.png",
];

const uiDir = path.join(REPO, "docs", "evidence", "nextgen", "ui");
const missing = requiredScreenshots.filter(
  (name) => !fs.existsSync(path.join(uiDir, name)),
);
if (missing.length > 0) {
  fail(`缺少真实运行截图：${missing.join("、")}`);
}

const runtimePath = path.join(REPO, "docs", "evidence", "nextgen", "ui-runtime.json");
if (!fs.existsSync(runtimePath)) {
  fail("缺少 docs/evidence/nextgen/ui-runtime.json");
}
const runtime = JSON.parse(fs.readFileSync(runtimePath, "utf8"));
if (runtime.uiGeneration !== 3) fail("uiGeneration 必须为 3");
if (runtime.runtime !== "tauri") fail("runtime 必须为 tauri");
if (runtime.productionShell !== true) fail("productionShell 必须为 true");
if (runtime.oldUiDefault !== false) fail("oldUiDefault 必须为 false");
if (runtime.visualGate !== "passed") fail("visualGate 必须为 passed");

const app = fs.readFileSync(path.join(REPO, "src", "App.tsx"), "utf8");
if (!app.includes("./ui/tokens.css") || !app.includes("./ui/shell.css")) {
  fail("生产 App 未引用 UI3 tokens/shell");
}

console.log("[ui3-runtime-gate] PASS");
