// BUILD_GATE：pnpm lint / test / build + cargo fmt / clippy -D warnings / test。
import { spawnSync } from "node:child_process";

const fail = (name, output) => {
  console.error(`[build-gate] FAIL: ${name}`);
  if (output?.stdout) process.stdout.write(output.stdout);
  if (output?.stderr) process.stderr.write(output.stderr);
  process.exit(1);
};

const run = (cmd, args, cwd = process.cwd()) =>
  spawnSync(cmd, args, {
    cwd,
    stdio: "pipe",
    encoding: "utf8",
  });

const pnpm = process.platform === "win32" ? "pnpm.cmd" : "pnpm";

const checks = [
  ["pnpm lint", pnpm, ["lint"], process.cwd()],
  ["pnpm test", pnpm, ["test"], process.cwd()],
  ["pnpm build", pnpm, ["build"], process.cwd()],
  ["cargo fmt --check", "cargo", ["fmt", "--check"], "src-tauri"],
  ["cargo clippy -D warnings", "cargo", ["clippy", "--all-targets", "--all-features", "--", "-D", "warnings"], "src-tauri"],
  ["cargo test --all-targets", "cargo", ["test", "--all-targets"], "src-tauri"],
];

for (const [name, cmd, args, cwd] of checks) {
  const result = run(cmd, args, cwd);
  if (result.status !== 0) fail(name, result);
  console.log(`[build-gate] PASS ${name}`);
}
console.log("[build-gate] PASS");
