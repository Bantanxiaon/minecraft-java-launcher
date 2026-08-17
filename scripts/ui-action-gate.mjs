// UI Action Gate (honest version)
// 1) Static scan: no empty onClick / console-only handlers / href="#" / TODO placeholders.
// 2) Action matrix: every entry must have a real handler, a real backend command or
//    route (or an explicit frontend behavior), a valid status, and test evidence.
// BROKEN/PLACEHOLDER/UNTESTED_PRIMARY_ACTION must all be 0 for PASS.
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const SRC = path.join(REPO, "src");
const RUST = path.join(REPO, "src-tauri", "src");
const MATRIX = path.join(REPO, "docs", "nextgen", "ui-actions.json");
const EVIDENCE_DIR = path.join(REPO, "docs", "evidence", "nextgen");

const walk = (dir) => {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(full));
    else if (/\.(tsx|ts)$/.test(entry.name)) out.push(full);
  }
  return out;
};

const fail = (message) => {
  console.error(`[ui-action-gate] FAIL: ${message}`);
  process.exit(1);
};

const files = walk(SRC);
const fileContents = new Map(
  files.map((file) => [
    path.relative(REPO, file).replace(/\\/g, "/"),
    fs.readFileSync(file, "utf8"),
  ]),
);
const allSource = [...fileContents.values()].join("\n");
const rustFiles = fs
  .readdirSync(RUST, { recursive: true, withFileTypes: true })
  .filter((entry) => entry.isFile() && entry.name.endsWith(".rs"))
  .map((entry) =>
    fs.readFileSync(path.join(entry.parentPath ?? RUST, entry.name), "utf8"),
  );
const rustSource = rustFiles.join("\n");

// ---- 1. static scan ----
const findings = [];
const patterns = [
  { id: "EMPTY_ONCLICK", regex: /onClick=\{\(\)\s*=>\s*\{\s*\}\}/g },
  { id: "CONSOLE_ONLY_HANDLER", regex: /onClick=\{\(\)\s*=>\s*console\.log\(/g },
  { id: "HASH_LINK", regex: /href=["']#["']/g },
  {
    id: "PLACEHOLDER_TEXT",
    regex: /(暂未实现|敬请期待|coming soon|TODO)[^>]*<\/button>|onClick=\{\(\)\s*=>\s*(alert\(["']暂未实现|return;\s*\/\/\s*TODO)/gi,
  },
  { id: "NOOP_VOID", regex: /onClick=\{\(\)\s*=>\s*void\s+0\}/g },
];
for (const [relative, content] of fileContents) {
  for (const pattern of patterns) {
    for (const match of content.matchAll(pattern.regex)) {
      const line = content.slice(0, match.index).split("\n").length;
      findings.push({
        id: pattern.id,
        file: relative,
        line,
        snippet: match[0].slice(0, 100),
      });
    }
  }
}

// ---- 2. action matrix ----
if (!fs.existsSync(MATRIX)) fail(`缺少 ${MATRIX}`);
const actions = JSON.parse(fs.readFileSync(MATRIX, "utf8"));
if (!Array.isArray(actions) || actions.length < 20) {
  fail("ui-actions.json 不是有效的动作矩阵");
}

const ALLOWED_STATUS = new Set([
  "IMPLEMENTED",
  "HIDDEN_BY_FEATURE_FLAG",
  "DISABLED_WITH_RUNTIME_REASON",
]);

const PRIMARY_ACTIONS = new Set([
  "home.launch",
  "library.play",
  "instance.launch",
  "home.createInstance",
  "settings.save",
  "accounts.createOffline",
  "downloads.cancel",
  "instance.mod.toggle",
  "instance.world.backup",
  "discover.modpack.import",
  "settings.loginExternal",
  "instance.log.read",
]);

const FRONTEND_HANDLERS = new Set([
  "navigate",
  "setShowChangelog",
  "setShowTutorial",
  "setShowOnboarding",
  "setSelectedJob",
  "setSelectedInstanceId",
  "setDiscoverTab",
  "setSettingsTab",
  "setContentKind",
  "changeTab",
  "changeDownloadMode",
  "setThemeMode",
  "setShowDownloadDetails",
  "loadLogs",
  "readLog",
  "retry",
  "onSelectAccount",
  "runWindowAction",
  "openInstanceForm",
  "createInstance",
  "selectAccount",
]);

const broken = [];
const untestedPrimary = [];
let implemented = 0;
let hidden = 0;
let disabled = 0;

for (const action of actions) {
  if (!ALLOWED_STATUS.has(action.status)) {
    broken.push(`${action.actionId}: 非法状态 ${action.status}`);
    continue;
  }
  if (action.status === "HIDDEN_BY_FEATURE_FLAG") hidden++;
  if (action.status === "DISABLED_WITH_RUNTIME_REASON") disabled++;
  if (action.status !== "IMPLEMENTED") continue;
  implemented++;

  if (!action.handler || typeof action.handler !== "string") {
    broken.push(`${action.actionId}: 缺少 handler`);
    continue;
  }
  const handlerTokens = action.handler
    .split(/[\/,]/)
    .map((token) => token.trim())
    .filter(Boolean);
  for (const token of handlerTokens) {
    if (FRONTEND_HANDLERS.has(token)) continue;
    if (!allSource.includes(token)) {
      broken.push(`${action.actionId}: handler "${token}" 未在 src 中找到`);
    }
  }

  const backend = action.backendCommandOrRoute ?? "";
  if (backend.startsWith("invoke:")) {
    const command = backend.slice("invoke:".length).trim();
    const isWindowApi = /^window\./.test(command);
    if (!isWindowApi && !rustSource.includes(command)) {
      broken.push(`${action.actionId}: backend command "${command}" 未在 Rust 源码中找到`);
    }
  } else if (backend.startsWith("route:")) {
    const routeName = backend.slice("route:".length).trim();
    const router = fs.readFileSync(
      path.join(REPO, "src", "app", "Router.tsx"),
      "utf8",
    );
    if (routeName !== "*" && !router.includes(routeName)) {
      broken.push(`${action.actionId}: route "${routeName}" 未在 Router.tsx 中找到`);
    }
  } else if (!backend.startsWith("frontend:")) {
    broken.push(
      `${action.actionId}: backendCommandOrRoute 必须以 invoke:/route:/frontend: 开头`,
    );
  }

  if (!action.testId) {
    broken.push(`${action.actionId}: 缺少 testId`);
  } else if (PRIMARY_ACTIONS.has(action.actionId)) {
    if (action.testId.startsWith("evidence:")) {
      const evidencePath = path.join(
        EVIDENCE_DIR,
        action.testId.slice("evidence:".length),
      );
      if (!fs.existsSync(evidencePath)) {
        untestedPrimary.push(`${action.actionId}: 缺少证据 ${evidencePath}`);
      }
    } else if (action.testId.startsWith("screenshot:")) {
      const screenshotPath = path.join(
        EVIDENCE_DIR,
        "ui-after",
        action.testId.slice("screenshot:".length),
      );
      if (!fs.existsSync(screenshotPath)) {
        untestedPrimary.push(`${action.actionId}: 缺少截图 ${screenshotPath}`);
      }
    } else {
      untestedPrimary.push(
        `${action.actionId}: 主动作必须引用 evidence:/screenshot: 证据`,
      );
    }
  }
}

const total = actions.length;
const placeholder = findings.filter((f) => f.id === "PLACEHOLDER_TEXT").length;
const allBroken = findings.length + broken.length;

const result = {
  scannedFiles: files.length,
  totalInteractiveControls: total,
  implemented,
  hiddenByFeatureFlag: hidden,
  disabledWithRuntimeReason: disabled,
  broken: allBroken,
  placeholder,
  untestedPrimaryActions: untestedPrimary.length,
  staticFindings: findings,
  matrixFindings: broken,
  untestedPrimary,
  status: allBroken === 0 && untestedPrimary.length === 0 ? "PASS" : "FAIL",
  generatedAt: new Date().toISOString(),
};
fs.mkdirSync(EVIDENCE_DIR, { recursive: true });
fs.writeFileSync(
  path.join(EVIDENCE_DIR, "ui-action-gate.json"),
  JSON.stringify(result, null, 2) + "\n",
);

if (allBroken > 0 || untestedPrimary.length > 0) {
  for (const finding of findings.slice(0, 15)) {
    console.error(
      `  ${finding.id} ${finding.file}:${finding.line} ${finding.snippet}`,
    );
  }
  for (const finding of broken.slice(0, 15)) console.error(`  ${finding}`);
  for (const finding of untestedPrimary.slice(0, 15)) {
    console.error(`  ${finding}`);
  }
  console.error(
    `[ui-action-gate] FAIL: broken=${allBroken} placeholder=${placeholder} untestedPrimary=${untestedPrimary.length}`,
  );
  process.exit(1);
}
console.log(
  `[ui-action-gate] PASS (actions=${total} implemented=${implemented} hidden=${hidden} disabled=${disabled} broken=0 placeholder=0 untestedPrimary=0)`,
);
