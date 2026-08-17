// DEPENDENCY_RESOLUTION_GATE
// 依赖解析：exact project identity + 版本选择 + kotlinforforge/expandability 回归。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[dependency-resolution-gate] FAIL: ${message}`);
  process.exit(1);
};

const libRs = fs.readFileSync(path.join(REPO, "src-tauri", "src", "lib.rs"), "utf8");
for (const needle of [
  "resolve_modrinth_project_id",
  "modrinth_project_by_hash",
  "trusted_aliases",
  "auto_install_missing_mod_dependencies",
  "dependency_resolver_regression_expandability_and_kotlinforforge",
  "missing_dependencies_reports_kotlinforforge_when_absent",
]) {
  if (!libRs.includes(needle)) fail(`缺少依赖解析实现/回归：${needle}`);
}

const evidence = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "dependency-regression.json",
);
if (!fs.existsSync(evidence)) {
  fail("缺少 dependency-regression.json（真实 live 回归证据）");
}
const parsed = JSON.parse(fs.readFileSync(evidence, "utf8"));
if (parsed.expandability?.status !== "passed" || parsed.kotlinforforge?.status !== "passed") {
  fail("expandability / kotlinforforge 回归必须真实通过");
}

console.log("[dependency-resolution-gate] PASS");
