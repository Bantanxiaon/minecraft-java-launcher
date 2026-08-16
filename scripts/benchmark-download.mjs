// 下载基准工具（需在本机运行）：对指定 URL 列表做冷/热缓存下载测速。
// 用法：node scripts/benchmark-download.mjs --url <url> --sha1 <sha1> [--repeat 3] [--out benchmark.json]
// PCL 对照：在 PCL2 中安装同一整合包并记录总耗时，填入 benchmark.json 的 pclSeconds 字段。

import { createHash } from "node:crypto";
import { createWriteStream, promises as fs } from "node:fs";
import path from "node:path";
import os from "node:os";

const args = process.argv.slice(2);
const get = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};

const url = get("--url");
const expectedSha1 = get("--sha1");
const repeat = Number(get("--repeat") ?? 3);
const output = get("--out");

if (!url) {
  console.error("缺少 --url");
  process.exit(2);
}

async function downloadOnce(target) {
  const started = performance.now();
  const response = await fetch(url, { redirect: "follow" });
  if (!response.ok || !response.body) {
    throw new Error(`HTTP ${response.status}`);
  }
  const file = createWriteStream(target);
  const reader = response.body.getReader();
  const hasher = createHash("sha1");
  let bytes = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    bytes += value.byteLength;
    hasher.update(value);
    file.write(Buffer.from(value));
  }
  await new Promise((resolve, reject) => file.end(resolve).on("error", reject));
  const elapsed = (performance.now() - started) / 1000;
  const sha1 = hasher.digest("hex");
  if (expectedSha1 && sha1.toLowerCase() !== expectedSha1.toLowerCase()) {
    throw new Error(`SHA-1 不匹配：${sha1}`);
  }
  return { bytes, elapsedSeconds: elapsed, bytesPerSecond: bytes / elapsed };
}

const temp = await fs.mkdtemp(path.join(os.tmpdir(), "sh-bench-"));
const target = path.join(temp, "bench.bin");
const runs = [];
for (let index = 0; index < repeat; index += 1) {
  await fs.rm(target, { force: true });
  runs.push(await downloadOnce(target));
  console.log(`run ${index + 1}: ${runs[index].bytesPerSecond / 1024 / 1024} MB/s`);
}
const median = runs.map((run) => run.bytesPerSecond).sort((a, b) => a - b)[Math.floor(runs.length / 2)];
const report = {
  url,
  repeat,
  runs,
  medianBytesPerSecond: median,
  note: "如需 PCL 对照，请在同一台电脑、同一网络安装同一整合包，记录总耗时并写入 pclSeconds。",
};
if (output) {
  await fs.writeFile(output, JSON.stringify(report, null, 2) + "\n");
} else {
  console.log(JSON.stringify(report, null, 2));
}
await fs.rm(temp, { recursive: true, force: true });
