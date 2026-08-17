// TYPOGRAPHY_RHYTHM_GATE：人工验收 + token 约束。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[typography-rhythm-gate] FAIL: ${message}`);
  process.exit(1);
};

const tokens = fs.readFileSync(path.join(REPO, "src", "ui", "tokens.css"), "utf8");
for (const token of [
  "--weight-normal: 400",
  "--weight-medium: 500",
  "--weight-semibold: 600",
  "--font-display:",
]) {
  if (!tokens.includes(token)) fail(`缺少字体 token：${token}`);
}

const review = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "UI_VISUAL_REVIEW_V2.md",
);
if (!fs.existsSync(review)) {
  fail("缺少 UI_VISUAL_REVIEW_V2.md，无法确认 TYPOGRAPHY_RHYTHM 人工验收");
}
if (!/TYPOGRAPHY_RHYTHM_GATE\s*[|:]\s*PASS/i.test(fs.readFileSync(review, "utf8"))) {
  fail("UI_VISUAL_REVIEW_V2.md 中 TYPOGRAPHY_RHYTHM_GATE 未标注 PASS");
}
console.log("[typography-rhythm-gate] PASS");
