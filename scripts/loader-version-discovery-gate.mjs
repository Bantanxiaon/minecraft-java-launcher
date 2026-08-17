// LOADER_VERSION_DISCOVERY_GATE
// 真实证据：官方元数据缓存 JSON + 动态命令存在 + 无硬编码 Loader build 数组。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[loader-version-discovery-gate] FAIL: ${message}`);
  process.exit(1);
};

const evidenceDir = path.join(
  REPO,
  "docs",
  "evidence",
  "nextgen",
  "loader-metadata",
);

const required = [
  ["forge-1.20.1.json", 60],
  ["neoforge-1.20.1.json", 20],
  ["fabric-1.20.1.json", 100],
  ["quilt-1.20.1.json", 100],
];
for (const [name, minCount] of required) {
  const file = path.join(evidenceDir, name);
  if (!fs.existsSync(file)) {
    fail(`缺少真实元数据缓存证据：${name}`);
  }
  const parsed = JSON.parse(fs.readFileSync(file, "utf8"));
  const count = Array.isArray(parsed.versions) ? parsed.versions.length : 0;
  if (count < minCount) {
    fail(`${name} 版本数过少（${count} < ${minCount}），疑似静态少量版本`);
  }
}

const libRs = fs.readFileSync(path.join(REPO, "src-tauri", "src", "lib.rs"), "utf8");
if (!libRs.includes("async fn list_loader_builds")) {
  fail("缺少动态 list_loader_builds 命令");
}
if (!libRs.includes("write_loader_cache") || !libRs.includes("read_loader_cache")) {
  fail("缺少 Loader 元数据缓存/离线回退实现");
}

// 禁止核心 Loader 使用少量静态 build 数组
const srcFiles = [];
const walk = (dir) => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (/\.(ts|tsx|rs)$/.test(entry.name)) srcFiles.push(full);
  }
};
walk(path.join(REPO, "src"));
walk(path.join(REPO, "src-tauri", "src"));
for (const file of srcFiles) {
  const content = fs.readFileSync(file, "utf8");
  const hardcoded = content.match(
    /\["[0-9]+\.[0-9]+\.[0-9]+"[^\]]{0,120}"[0-9]+\.[0-9]+\.[0-9]+"\]/,
  );
  if (hardcoded && /loader|forge|fabric|quilt|neoforge/i.test(content.slice(0, 2000))) {
    fail(`疑似硬编码 Loader build 数组：${path.relative(REPO, file)}`);
  }
}

console.log("[loader-version-discovery-gate] PASS");
