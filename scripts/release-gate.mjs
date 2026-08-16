// 机器级发布门禁：任何一项失败退出 1，阻止 tag / Release / latest.json / updater。

import fs from "node:fs";

const fail = (message) => {
  console.error(`[release-gate] FAIL: ${message}`);
  process.exit(1);
};

const read = (path) => {
  try {
    return fs.readFileSync(path, "utf8");
  } catch {
    return null;
  }
};

// 1. 零遗留核对表：不允许存在任何未勾选项。
const checklist = read("docs/V081_ZERO_REMAINDER_CHECKLIST.md");
if (!checklist) fail("缺少 docs/V081_ZERO_REMAINDER_CHECKLIST.md");
const unchecked = checklist.split("\n").filter((line) => /^\s*-\s*\[ \]/.test(line));
if (unchecked.length > 0) {
  fail(`核对表仍有 ${unchecked.length} 个未完成项，禁止发布。`);
}

// 2. 版本一致性：package.json / tauri.conf.json / Cargo.toml 必须一致。
const pkg = JSON.parse(read("package.json") ?? "{}");
const tauri = JSON.parse(read("src-tauri/tauri.conf.json") ?? "{}");
const cargo = read("src-tauri/Cargo.toml") ?? "";
const cargoVersion = cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
if (pkg.version !== tauri.version || pkg.version !== cargoVersion) {
  fail(`版本不一致：package=${pkg.version} tauri=${tauri.version} cargo=${cargoVersion}`);
}

// 3. Release notes 与 changelog 必须存在。
if (!read(`docs/release-notes/${pkg.version}.md`)) {
  fail(`缺少 docs/release-notes/${pkg.version}.md`);
}
if (!(read("CHANGELOG.md") ?? "").includes(pkg.version)) {
  fail(`CHANGELOG.md 缺少版本 ${pkg.version}`);
}

// 4. 真实下载基准报告必须存在且包含有效数据。
const benchmark = read("docs/benchmark-download.json");
if (!benchmark) fail("缺少 docs/benchmark-download.json（真实下载基准）");
const parsed = JSON.parse(benchmark);
if (!parsed || typeof parsed !== "object" || Object.keys(parsed).length === 0) {
  fail("下载基准报告为空或无效");
}

// 5. Git 工作区必须干净（发布前由调用方保证）。
console.log("[release-gate] PASS");
