// BORDER_DENSITY_GATE：人工验收 + 样式密度约束。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[border-density-gate] FAIL: ${message}`);
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
  fail("缺少 UI_VISUAL_REVIEW_V2.md，无法确认 BORDER_DENSITY 人工验收");
}
const text = fs.readFileSync(review, "utf8");
if (!/BORDER_DENSITY_GATE\s*[|:]\s*PASS/i.test(text)) {
  fail("UI_VISUAL_REVIEW_V2.md 中 BORDER_DENSITY_GATE 未标注 PASS");
}

// 静态约束：主要页面卡片默认无边框（由 polish.css 层实现）
const polish = fs.readFileSync(path.join(REPO, "src", "ui", "polish.css"), "utf8");
if (!polish.includes(".home-grid > section.ui3-card") || !polish.includes("border: 0")) {
  fail("缺少低边框覆盖层（home/settings 卡片应去边框）");
}
console.log("[border-density-gate] PASS");
