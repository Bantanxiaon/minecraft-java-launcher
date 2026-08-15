import { readFile, writeFile } from "node:fs/promises";

const repository = process.env.GITHUB_REPOSITORY?.trim();
if (!repository || !/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository)) {
  throw new Error("GITHUB_REPOSITORY 格式无效，应为“账号/仓库”。");
}

const configPath = new URL("../src-tauri/tauri.conf.json", import.meta.url);
const config = JSON.parse(await readFile(configPath, "utf8"));
config.plugins ??= {};
config.plugins.updater ??= {};
config.plugins.updater.endpoints = [
  "https://github.com/" + repository + "/releases/latest/download/latest.json",
];
await writeFile(configPath, JSON.stringify(config, null, 2) + "\n", "utf8");
console.log("Updater endpoint configured for " + repository);
