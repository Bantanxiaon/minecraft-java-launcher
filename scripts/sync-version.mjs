import fs from "node:fs";

const version = process.argv[2];
if (!version || !/^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$/.test(version)) {
  throw new Error(`invalid version: ${version}`);
}

const writeJson = (path, value) =>
  fs.writeFileSync(path, JSON.stringify(value, null, 2) + "\n");

const pkgPath = "package.json";
const cargoPath = "src-tauri/Cargo.toml";
const tauriPath = "src-tauri/tauri.conf.json";

const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
pkg.version = version;
writeJson(pkgPath, pkg);

const tauri = JSON.parse(fs.readFileSync(tauriPath, "utf8"));
tauri.version = version;
writeJson(tauriPath, tauri);

let cargo = fs.readFileSync(cargoPath, "utf8");
cargo = cargo.replace(/^version\s*=\s*"[^"]+"/m, `version = "${version}"`);
fs.writeFileSync(cargoPath, cargo);

console.log(`version synced to ${version}`);
