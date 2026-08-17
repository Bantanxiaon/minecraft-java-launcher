// STARTUP_WINDOW_GATE：show ACK != visible 仍必须 FAIL，必须 MAIN_VISIBLE_CONFIRMED。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[startup-window-gate] FAIL: ${message}`);
  process.exit(1);
};

const candidates = [
  path.join(REPO, "docs", "evidence", "nextgen", "startup-window.json"),
  path.join(REPO, "docs", "evidence", "startup-window.json"),
  path.join(REPO, "docs", "evidence", "window-acceptance.json"),
];
const file = candidates.find((candidate) => fs.existsSync(candidate));
if (!file) fail("缺少 startup-window 真实证据（show ACK != visible 直接 FAIL）");

const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
if (parsed.mainVisibleConfirmed !== true) {
  fail("MAIN_VISIBLE_CONFIRMED 必须为 true；仅收到 show ACK 不算通过");
}
console.log("[startup-window-gate] PASS");
