// UI3_RUNTIME_GATE (honest version):
// - production root (App.tsx) 必须引用 UI3 shell，且不得导入旧 CSS / ServersPage
// - 必需页面必须存在于 src/features
// - 真实运行截图必须齐全（ui-before / ui-after）
// - ui-runtime.json 字段必须为真
// - UI Action Gate 结果必须为 PASS
// - UI_BEFORE_AFTER_REPORT.md 必须存在
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[ui3-runtime-gate] FAIL: ${message}`);
  process.exit(1);
};

const appTsx = fs.readFileSync(path.join(REPO, "src", "App.tsx"), "utf8");
if (!appTsx.includes("AppShell")) fail("生产 App 未引用 UI3 AppShell");
if (appTsx.includes("ServersPage")) {
  fail("生产 App 仍引用 ServersPage（联机入口未退役）");
}
for (const legacy of ["./App.css", "./overrides.css", "ui2.css", "./ui2.css"]) {
  if (appTsx.includes(legacy)) fail(`生产 App 仍导入旧 CSS：${legacy}`);
}

const requiredPages = [
  "features/home/HomePage.tsx",
  "features/library/LibraryPage.tsx",
  "features/instance/InstancePage.tsx",
  "features/discover/DiscoverPage.tsx",
  "features/downloads/DownloadsPage.tsx",
  "features/accounts/AccountsPage.tsx",
  "features/settings/SettingsPage.tsx",
];
for (const page of requiredPages) {
  if (!fs.existsSync(path.join(REPO, "src", page))) {
    fail(`缺少必需页面：src/${page}`);
  }
}

const beforeDir = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "ui-before",
);
const afterDir = path.join(REPO, "docs", "evidence", "nextgen", "ui-after");
const requiredBefore = [
  "home.png",
  "library.png",
  "instance.png",
  "downloads.png",
  "accounts.png",
  "settings.png",
];
const requiredAfter = [
  "splash.png",
  "home.png",
  "library.png",
  "instance-overview.png",
  "instance-mods.png",
  "instance-worlds.png",
  "discover.png",
  "downloads.png",
  "accounts.png",
  "settings-general.png",
  "settings-java.png",
  "settings-download.png",
  "storage.png",
  "diagnostics.png",
  "error-state.png",
  "dialog.png",
];
const missingBefore = requiredBefore.filter(
  (name) => !fs.existsSync(path.join(beforeDir, name)),
);
const missingAfter = requiredAfter.filter(
  (name) => !fs.existsSync(path.join(afterDir, name)),
);
if (missingBefore.length) fail(`缺少 before 截图：${missingBefore.join("、")}`);
if (missingAfter.length) fail(`缺少 after 截图：${missingAfter.join("、")}`);

const runtimePath = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "ui-runtime.json",
);
if (!fs.existsSync(runtimePath)) fail("缺少 docs/evidence/nextgen/ui-runtime.json");
const runtime = JSON.parse(fs.readFileSync(runtimePath, "utf8"));
if (runtime.uiGeneration !== 3) fail("uiGeneration 必须为 3");
if (runtime.runtime !== "tauri") fail("runtime 必须为 tauri");
if (runtime.productionShell !== true) fail("productionShell 必须为 true");
if (runtime.oldUiDefault !== false) fail("oldUiDefault 必须为 false");
if (!["passed", "manual_review_required"].includes(runtime.visualGate)) {
  fail("visualGate 必须为 passed 或 manual_review_required（主观视觉由单独 Gate 判定）");
}

if (
  !fs.existsSync(
    path.join(
      REPO,
      "docs",
      "evidence",
      "nextgen",
      "UI_BEFORE_AFTER_REPORT.md",
    ),
  )
) {
  fail("缺少 UI_BEFORE_AFTER_REPORT.md");
}

const actionGatePath = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "ui-action-gate.json",
);
if (!fs.existsSync(actionGatePath)) fail("缺少 ui-action-gate.json 结果");
const actionGate = JSON.parse(fs.readFileSync(actionGatePath, "utf8"));
if (
  actionGate.status !== "PASS" ||
  actionGate.broken !== 0 ||
  actionGate.placeholder !== 0
) {
  fail("UI Action Gate 未通过（BROKEN/PLACEHOLDER 非零）");
}

console.log("[ui3-runtime-gate] PASS");
