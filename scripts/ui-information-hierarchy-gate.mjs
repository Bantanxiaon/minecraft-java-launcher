// UI_INFORMATION_HIERARCHY_GATE
// 普通用户主界面不得默认显示 raw URL/hash/exit code/internal status。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[ui-information-hierarchy-gate] FAIL: ${message}`);
  process.exit(1);
};

const scanDirs = [
  path.join(REPO, "src", "features"),
  path.join(REPO, "src", "app"),
  path.join(REPO, "src", "components", "OnboardingGuide.tsx"),
  path.join(REPO, "src", "components", "TutorialModal.tsx"),
  path.join(REPO, "src", "components", "SplashScreen.tsx"),
];
const patterns = [
  { id: "RAW_SOURCE_URL", regex: /\{job\.sourceUrl\}|\{item\.sourceUrl\}|job\.sourceUrl\s*<\//g },
  { id: "RAW_HASH", regex: /\{expectedHash\}|\{item\.hash\.slice|sha256\.slice\(0, 20\}\)/g },
  { id: "RAW_EXIT_CODE", regex: /退出码\s*\$|exitCode\s*}/g },
  { id: "DEBUG_LOADER_TEXT", regex: /检查模组说明文件|Fabric 检查说明文件|联网漂移/g },
  { id: "INTERNAL_STATUS", regex: /implementation status|debug loader descriptor|internal job id/g },
];

const files = [];
for (const target of scanDirs) {
  if (!fs.existsSync(target)) continue;
  const stat = fs.statSync(target);
  if (stat.isFile()) {
    files.push(target);
    continue;
  }
  const walk = (dir) => {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith(".")) continue;
      const full = path.join(dir, entry.name);
      if (entry.isDirectory()) walk(full);
      else if (/\.(tsx|ts)$/.test(entry.name)) files.push(full);
    }
  };
  walk(target);
}

const findings = [];
for (const file of files) {
  const content = fs.readFileSync(file, "utf8");
  for (const pattern of patterns) {
    for (const match of content.matchAll(pattern.regex)) {
      const line = content.slice(0, match.index).split("\n").length;
      findings.push(
        `${pattern.id} ${path.relative(REPO, file)}:${line} ${match[0].slice(0, 80)}`,
      );
    }
  }
}
if (findings.length) {
  fail(`主界面泄漏技术字段：${findings.slice(0, 8).join("；")}`);
}
console.log("[ui-information-hierarchy-gate] PASS");
