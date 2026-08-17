// UI Action Gate：静态扫描生产 React 源码中的无效交互（NextGen §9.1）。
// 退出码 0 = PASS；任何 finding 都导致 FAIL（BROKEN/PLACEHOLDER 不允许发布）。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const SRC = path.join(REPO, "src");
const OUT = path.join(REPO, "docs", "nextgen");
fs.mkdirSync(OUT, { recursive: true });

const files = [];
const walk = (dir) => {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith(".") || entry.name === "node_modules") continue;
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) walk(full);
    else if (entry.name.endsWith(".tsx")) files.push(full);
  }
};
walk(SRC);

const findings = [];
const patterns = [
  {
    id: "EMPTY_ONCLICK",
    label: "空 onClick",
    regex: /onClick=\{\(\)\s*=>\s*\{\s*\}\}/g,
  },
  {
    id: "CONSOLE_ONLY_HANDLER",
    label: "仅 console.log 的处理器",
    regex: /onClick=\{\(\)\s*=>\s*console\.log\(/g,
  },
  {
    id: "HASH_LINK",
    label: "href=\"#\" 假链接",
    regex: /href=["']#["']/g,
  },
  {
    id: "PLACEHOLDER_TEXT",
    label: "占位文案绑在控件上",
    regex: /(暂未实现|敬请期待|coming soon|TODO)[^>]*<\/button>|onClick=\{\(\)\s*=>\s*(alert\(["']暂未实现|return;\s*\/\/\s*TODO)/gi,
  },
  {
    id: "NOOP_VOID",
    label: "void 0 空操作",
    regex: /onClick=\{\(\)\s*=>\s*void\s+0\}/g,
  },
];

for (const file of files) {
  const content = fs.readFileSync(file, "utf8");
  const relative = path.relative(REPO, file).replace(/\\/g, "/");
  for (const pattern of patterns) {
    for (const match of content.matchAll(pattern.regex)) {
      const before = content.slice(0, match.index).split("\n");
      findings.push({
        id: pattern.id,
        label: pattern.label,
        file: relative,
        line: before.length,
        snippet: match[0].slice(0, 120),
      });
    }
  }
}

const evidence = {
  scannedFiles: files.length,
  findings,
  totalInteractiveControls: findings.length,
  broken: findings.length,
  placeholder: findings.filter((f) => f.id === "PLACEHOLDER_TEXT").length,
  status: findings.length === 0 ? "PASS" : "FAIL",
  generatedAt: new Date().toISOString(),
};
fs.writeFileSync(path.join(OUT, "ui-actions.json"), JSON.stringify(evidence, null, 2) + "\n");

if (findings.length > 0) {
  console.error(`[ui-action-gate] FAIL: ${findings.length} findings`);
  for (const finding of findings.slice(0, 20)) {
    console.error(`  ${finding.id} ${finding.file}:${finding.line} ${finding.snippet}`);
  }
  process.exit(1);
}
console.log(`[ui-action-gate] PASS (${files.length} files scanned, 0 broken/placeholder)`);
