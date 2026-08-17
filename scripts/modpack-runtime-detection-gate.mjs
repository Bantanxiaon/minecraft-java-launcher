// MODPACK_RUNTIME_DETECTION_GATE
// 必须保留 exact Loader version，Manifest 优先，通用 ZIP 才有 confidence fallback。
import fs from "node:fs";
import path from "node:path";

const REPO = process.cwd();
const fail = (message) => {
  console.error(`[modpack-runtime-detection-gate] FAIL: ${message}`);
  process.exit(1);
};

const libRs = fs.readFileSync(path.join(REPO, "src-tauri", "src", "lib.rs"), "utf8");
if (!libRs.includes("loader_version: Option<String>")) {
  fail("ModpackInspection 缺少 loader_version 字段");
}
if (!libRs.includes("confidence: Option<f32>")) {
  fail("ModpackInspection 缺少 confidence 字段");
}
if (!libRs.includes("java_major: Option<u32>")) {
  fail("ModpackInspection 缺少 java_major 字段");
}
if (!libRs.includes('必须保留精确 Loader build，禁止升级')) {
  fail("缺少 exact Loader version 回归断言（synthetic_packs_are_detected_in_all_formats）");
}
if (!libRs.includes("detect_generic_pack_loader")) {
  fail("缺少通用 ZIP fallback 检测");
}

const typesTs = fs.readFileSync(path.join(REPO, "src", "types.ts"), "utf8");
if (!typesTs.includes("loaderVersion") || !typesTs.includes("confidence")) {
  fail("前端 ModpackInspection 类型缺少 loaderVersion/confidence");
}

console.log("[modpack-runtime-detection-gate] PASS");
