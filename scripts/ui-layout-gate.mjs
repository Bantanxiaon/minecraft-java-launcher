// UI_LAYOUT_INTEGRITY_GATE
// 生产 Tauri DOM getBoundingClientRect 审计证据必须存在；静态兜底检查禁止压缩换行。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[ui-layout-gate] FAIL: ${message}`);
  process.exit(1);
};

const evidence = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "ui-layout-audit.json",
);
if (!fs.existsSync(evidence)) {
  fail(
    "缺少 ui-layout-audit.json（生产 Tauri DOM 审计未执行：overlap/overflow/clipping 无法判定）",
  );
}
const audit = JSON.parse(fs.readFileSync(evidence, "utf8"));
if (audit.runtime !== "tauri" || audit.source !== "getBoundingClientRect") {
  fail("布局审计必须来自真实 Tauri DOM getBoundingClientRect");
}
if (!Array.isArray(audit.pages) || audit.pages.length < 6) {
  fail("布局审计必须覆盖 home/library/discover/downloads/accounts/settings 六页");
}
const requiredPages = ["home", "library", "discover", "downloads", "accounts", "settings"];
const issues = [];
for (const name of requiredPages) {
  const page = audit.pages.find((entry) => entry.name === name);
  if (!page) {
    fail(`布局审计缺少页面：${name}`);
  }
  issues.push(
    ...(page.overlaps ?? []).map((item) => `${name} overlap`),
    ...(page.overflows ?? []).map((item) => `${name} overflow`),
    ...(page.singleCharacterWraps ?? []).map((item) => `${name} wrap:${item.text}`),
    ...(page.horizontalScrolls ?? []).map(() => `${name} horizontal scroll`),
  );
}
if (issues.length) {
  fail(`检测到 ${issues.length} 个布局问题：${issues.slice(0, 5).join("、")}`);
}

// 静态兜底：禁止用 break-all/9px 掩盖压缩
for (const file of ["src/ui/pages.css", "src/ui/globals.css", "src/ui/polish.css"]) {
  const css = fs.readFileSync(path.join(REPO, file), "utf8");
  if (css.includes("font-size: 9px") || css.includes("word-break: break-all")) {
    fail(`${file} 存在掩盖文本压缩的样式`);
  }
}

console.log("[ui-layout-gate] PASS");
