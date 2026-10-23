"""Standalone development commands: python tools/dev.py check|run|info. Python 3.10+."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
config_path = ROOT / ".project" / "local-environment.json"
config = json.loads(config_path.read_text(encoding="utf-8")) if config_path.exists() else {}
dotnet = os.environ.get("SH_DOTNET") or config.get("dotnet") or shutil.which("dotnet")
if not dotnet:
    raise SystemExit("Install the .NET SDK specified by global.json or set SH_DOTNET.")
env = os.environ.copy()
env["DOTNET_ROOT"] = str(Path(dotnet).resolve().parent)
env["PATH"] = env["DOTNET_ROOT"] + os.pathsep + env.get("PATH", "")
env["DOTNET_CLI_TELEMETRY_OPTOUT"] = "1"
env["DOTNET_NOLOGO"] = "1"
env["DOTNET_GENERATE_ASPNET_CERTIFICATE"] = "false"
env["AVALONIA_TELEMETRY_OPTOUT"] = "1"
if config.get("cacheRoot"):
    cache = Path(config["cacheRoot"])
    for key, folder in [("DOTNET_CLI_HOME", "cli"), ("NUGET_PACKAGES", "packages"), ("NUGET_HTTP_CACHE_PATH", "http"), ("NUGET_PLUGINS_CACHE_PATH", "plugins"), ("XDG_CACHE_HOME", "xdg")]:
        location = cache / folder
        location.mkdir(parents=True, exist_ok=True)
        env[key] = str(location)
if config.get("temp"):
    Path(config["temp"]).mkdir(parents=True, exist_ok=True)
    env["TEMP"] = env["TMP"] = config["temp"]

def execute(*arguments):
    subprocess.run([dotnet, *arguments], cwd=ROOT, env=env, check=True)

command = sys.argv[1] if len(sys.argv) > 1 else "check"
if command == "check":
    execute("build", "SHLauncherNext.sln", "--no-incremental")
    for group in ("Core", "Application", "Platform", "Engine", "Downloads"):
        execute("run", "--project", f"tests/SHLauncher.{group}.Tests", "--no-build")
elif command == "run":
    env["SHLAUNCHER_DATA_DIR"] = config.get("dataRoot", str(ROOT / ".devdata" / "local-user"))
    if config.get("curseForgeConfigFile"):
        env["SH_CURSEFORGE_CONFIG_FILE"] = config["curseForgeConfigFile"]
    execute("run", "--project", "src/SHLauncher.UI")
elif command == "info":
    execute("--info")
else:
    raise SystemExit("Use check, run or info.")
