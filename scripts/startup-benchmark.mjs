// 启动基准（需在本机运行）：启动真实 Windows 构建 N 次，读取 startup-metrics.json 统计
// 冷启动总耗时（含 Tauri 窗口初始化前的 DB 迁移/恢复阶段）。
// 用法：node scripts/startup-benchmark.mjs --exe <path\to\sh-launcher.exe> [--runs 5] [--data <launcher-data-dir>]

import { spawn } from "node:child_process";
import { promises as fs } from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const get = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};

const exe = get("--exe");
const runs = Number(get("--runs") ?? 5);
const dataRoot =
  get("--data") ??
  path.join(process.env.LOCALAPPDATA ?? process.env.TEMP ?? ".", "SHLauncher");

if (!exe || !Number.isFinite(runs) || runs < 1) {
  console.error("缺少 --exe 或 --runs 无效");
  process.exit(2);
}

async function singleRun(index) {
  const metricsPath = path.join(dataRoot, "startup-metrics.json");
  await fs.rm(metricsPath, { force: true });
  const child = spawn(exe, [], {
    env: { ...process.env, SH_STARTUP_BENCH_EXIT: "1" },
    stdio: "ignore",
    windowsHide: true,
  });
  const started = Date.now();
  const deadline = started + 60_000;
  while (Date.now() < deadline) {
    try {
      const raw = await fs.readFile(metricsPath, "utf8");
      const metrics = JSON.parse(raw);
      const totalMs = Number(metrics.totalMs);
      if (Number.isFinite(totalMs)) {
        try {
          child.kill();
        } catch {
          // 已退出
        }
        return {
          run: index,
          totalMs,
          dbMs: Number(metrics.dbMs ?? 0),
          scaleFactor: Number(metrics.scaleFactor ?? 0),
          monitorCount: Number(metrics.monitorCount ?? 0),
        };
      }
    } catch {
      // 文件尚未写入
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  try {
    child.kill();
  } catch {
    // 已退出
  }
  throw new Error(`第 ${index} 次启动在 60 秒内未产出指标`);
}

const results = [];
for (let index = 1; index <= runs; index += 1) {
  const result = await singleRun(index);
  results.push(result);
  console.log(
    `run ${index}/${runs}: total=${result.totalMs}ms db=${result.dbMs}ms`,
  );
}

const totals = results.map((entry) => entry.totalMs).sort((a, b) => a - b);
const percentile = (values, ratio) => {
  const position = Math.max(0, Math.ceil(values.length * ratio) - 1);
  return values[position];
};
const summary = {
  generatedAt: new Date().toISOString(),
  exe,
  runs,
  samples: results,
  minMs: percentile(totals, 0),
  medianMs: percentile(totals, 0.5),
  p95Ms: percentile(totals, 0.95),
  maxMs: totals[totals.length - 1],
};

const outputJson = path.resolve(
  process.cwd(),
  "docs",
  "benchmark-startup.json",
);
await fs.mkdir(path.dirname(outputJson), { recursive: true });
await fs.writeFile(outputJson, `${JSON.stringify(summary, null, 2)}\n`);

const lines = [
  "# 启动基准（真实 Windows 构建）",
  "",
  `- 生成时间：${summary.generatedAt}`,
  `- 可执行文件：${exe}`,
  `- 采样次数：${runs}`,
  `- min / median / P95 / max：${summary.minMs}ms / ${summary.medianMs}ms / ${summary.p95Ms}ms / ${summary.maxMs}ms`,
  "",
  "| 运行 | 总耗时 ms | DB 阶段 ms |",
  "| --- | ---: | ---: |",
  ...results.map(
    (entry) => `| ${entry.run} | ${entry.totalMs} | ${entry.dbMs} |`,
  ),
  "",
];
const outputMd = path.resolve(
  process.cwd(),
  "docs",
  "BENCHMARK_STARTUP.md",
);
await fs.writeFile(outputMd, lines.join("\n"));
console.log(`summary: ${outputJson}`);
