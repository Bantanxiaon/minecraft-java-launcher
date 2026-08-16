// NextGen 基线清单：自动扫描仓库并输出 docs/nextgen/*.json
// 用法：node scripts/nextgen-baseline.mjs
import fs from "node:fs";
import path from "node:path";
import { execFileSync } from "node:child_process";

const REPO = process.cwd();
const OUT = path.join(REPO, "docs", "nextgen");
fs.mkdirSync(OUT, { recursive: true });

const read = (p) => {
  try {
    return fs.readFileSync(path.join(REPO, p), "utf8");
  } catch {
    return "";
  }
};

const files = (dir, ext) => {
  const out = [];
  const walk = (d) => {
    for (const entry of fs.readdirSync(d, { withFileTypes: true })) {
      if (entry.name.startsWith(".") || entry.name === "node_modules" || entry.name === "target" || entry.name === "dist") continue;
      const full = path.join(d, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (entry.name.endsWith(ext)) out.push(full.replace(/\\/g, "/"));
    }
  };
  walk(path.join(REPO, dir));
  return out;
};

const write = (name, data) => {
  fs.writeFileSync(path.join(OUT, name), JSON.stringify(data, null, 2) + "\n");
  console.log(`[baseline] ${name} (${Array.isArray(data) ? data.length : Object.keys(data).length} entries)`);
};

const pages = files("src", ".tsx")
  .filter((f) => /pages|components/.test(f))
  .sort();
const libRs = read("src-tauri/src/lib.rs");
const commands = [...libRs.matchAll(/#\[tauri::command\]\s*(?:async\s+)?fn\s+(\w+)/g)].map((m) => m[1]).sort();
const rustModules = files("src-tauri/src", ".rs").sort();
const migrations = [...libRs.matchAll(/ALTER TABLE|CREATE TABLE|CREATE (UNIQUE )?INDEX|PRAGMA/g)].map((m) => m[0]).sort();
const settingsKeys = [...libRs.matchAll(/settings\[['"]?([\w\-.]+)|settings\.(\w+)|microsoft_client_id|close_launcher_after_game_start/g)]
  .map((m) => m[1] || m[2])
  .filter(Boolean)
  .filter((v, i, a) => a.indexOf(v) === i)
  .sort();
const envVars = [...read("src-tauri/src/lib.rs").matchAll(/env::var(?:_os)?\("([A-Z0-9_]+)"/g)]
  .map((m) => m[1])
  .filter((v, i, a) => a.indexOf(v) === i)
  .sort();
const tests = files("src-tauri/src", ".rs")
  .flatMap((f) => [...read(f).matchAll(/#\[test\]\s+fn\s+(\w+)/g)].map((m) => `${f}:${m[1]}`))
  .sort();
const frontendTests = files("src", ".test.tsx").sort();
const workflows = files(".github/workflows", ".yml").sort();
const dbMigrations = [...libRs.matchAll(/migration_v?\w*|CURRENT_VERSION|SCHEMA_VERSION|user_version/g)]
  .map((m) => m[0])
  .filter((v, i, a) => a.indexOf(v) === i)
  .sort();

write("baseline-routes.json", pages);
write("baseline-tauri-commands.json", commands);
write("baseline-db-schema.json", {
  migrations: dbMigrations,
  ddlStatements: migrations,
});
write("baseline-settings.json", settingsKeys);
write("baseline-download-paths.json", []);
write("baseline-tests.json", {
  rust: tests,
  frontend: frontendTests,
});
write("baseline-release-pipeline.json", workflows);
write("baseline-features.json", {
  rustModules,
  pages,
  envVars,
  commands,
  generatedAt: new Date().toISOString(),
  head: execFileSync("git", ["rev-parse", "HEAD"], { cwd: REPO }).toString().trim(),
});

const matrix = [
  "## Feature Parity Matrix（基线 → NextGen）",
  "",
  "> 自动生成于 " + new Date().toISOString() + "。下一阶段逐项映射 NextGen 页面/命令/测试。",
  "",
  "| 基线能力 | 类型 | NextGen 映射 | 状态 |",
  "| --- | --- | --- | --- |",
  ...pages.map((p) => `| ${p} | page | TBD | MAPPING_PENDING |`),
  ...commands.map((c) => `| ${c} | command | TBD | MAPPING_PENDING |`),
  "",
  "规则：任何 `MAPPING_PENDING` 在最终 Release Gate 前必须变成 `IMPLEMENTED / EQUIVALENT / DEFERRED_BY_USER_MICROSOFT / HIDDEN_MULTIPLAYER / NOT_APPLICABLE_WITH_REASON`。",
  "",
].join("\n");
fs.writeFileSync(path.join(OUT, "FEATURE_PARITY_MATRIX.md"), matrix);
console.log("[baseline] FEATURE_PARITY_MATRIX.md written");
