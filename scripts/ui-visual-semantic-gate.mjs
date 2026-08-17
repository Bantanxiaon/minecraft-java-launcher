// UI_VISUAL_SEMANTIC_GATE：人工逐页评分，禁止脚本自评。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[ui-visual-semantic-gate] FAIL: ${message}`);
  process.exit(1);
};

const review = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "UI_VISUAL_REVIEW_V2.md",
);
if (!fs.existsSync(review)) {
  fail("缺少 UI_VISUAL_REVIEW_V2.md（人工逐页评分）→ MANUAL_REVIEW_REQUIRED");
}
const text = fs.readFileSync(review, "utf8");
const corePages = ["Home", "Tutorial", "Discover", "Downloads", "Settings", "Splash"];
const pageBlocks = text.split(/^## /m).slice(1);
const scored = [];
for (const block of pageBlocks) {
  const title = block.split("\n")[0].trim();
  const launcher = /Launcher Feel\s*[|:]\s*(\d+)/i.exec(block);
  const hierarchy = /Hierarchy\s*[|:]\s*(\d+)/i.exec(block);
  const typography = /Typography\s*[|:]\s*(\d+)/i.exec(block);
  if (launcher && hierarchy && typography) {
    scored.push({
      page: title,
      launcher: Number(launcher[1]),
      hierarchy: Number(hierarchy[1]),
      typography: Number(typography[1]),
    });
  }
}
if (scored.length < 10) {
  fail(`人工评分页数不足（${scored.length}/10+）→ MANUAL_REVIEW_REQUIRED`);
}
for (const page of corePages) {
  const entry = scored.find((item) => item.page.includes(page));
  if (!entry) {
    fail(`核心页 ${page} 缺少人工评分`);
  }
  if (entry.launcher < 7 || entry.hierarchy < 7 || entry.typography < 7) {
    fail(`${page}: Launcher/Hierarchy/Typography 存在 <7 分`);
  }
}
const all = scored.flatMap((item) => [
  item.launcher,
  item.hierarchy,
  item.typography,
]);
const average = all.reduce((sum, value) => sum + value, 0) / all.length;
if (average < 8) {
  fail(`总体平均 ${average.toFixed(2)} < 8`);
}
console.log(`[ui-visual-semantic-gate] PASS (pages=${scored.length} average=${average.toFixed(2)})`);
