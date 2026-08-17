// WORLD_CREATE_RUNTIME_GATE
// 四加载器真实创建世界 E2E；未执行 = FAIL（NOT_RUN_ENVIRONMENT_LIMITATION）。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[world-create-runtime-gate] FAIL: ${message}`);
  process.exit(1);
};

const dir = path.join(REPO, "docs", "evidence", "nextgen", "world-create");
const required = ["vanilla", "forge", "fabric", "neoforge"];
for (const loader of required) {
  const file = path.join(dir, `${loader}.json`);
  if (!fs.existsSync(file)) {
    fail(
      `${loader}: 缺少 world-create/${loader}.json（真实创建世界 E2E 未执行）→ NOT_RUN_ENVIRONMENT_LIMITATION`,
    );
  }
  const evidence = JSON.parse(fs.readFileSync(file, "utf8"));
  if (
    evidence.playerJoinMarker !== true ||
    evidence.stableSeconds < 60 ||
    evidence.saveExit !== true
  ) {
    fail(`${loader}: 创建世界 E2E 证据不完整或未通过`);
  }
}

console.log("[world-create-runtime-gate] PASS");
