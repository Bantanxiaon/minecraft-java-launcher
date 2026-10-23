"""Start the built local app independently, using the workspace's configured runtime and data directory."""
import json
import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
configuration = root / ".project" / "local-environment.json"
settings = json.loads(configuration.read_text(encoding="utf-8")) if configuration.exists() else {}
dotnet = os.environ.get("SH_DOTNET") or settings.get("dotnet") or shutil.which("dotnet")
application = root / "src" / "SHLauncher.UI" / "bin" / "Release" / "net10.0" / "SHLauncher.UI.dll"
if not dotnet or not application.is_file():
    raise SystemExit("Build Release first and configure the .NET runtime locally.")
environment = os.environ.copy()
environment["DOTNET_ROOT"] = str(Path(dotnet).resolve().parent)
environment["SHLAUNCHER_DATA_DIR"] = settings.get("dataRoot", str(root / ".devdata" / "local-user"))
if settings.get("curseForgeConfigFile"):
    environment["SH_CURSEFORGE_CONFIG_FILE"] = settings["curseForgeConfigFile"]
if settings.get("temp"):
    environment["TEMP"] = environment["TMP"] = settings["temp"]
flags = (subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS) if os.name == "nt" else 0
subprocess.Popen([dotnet, str(application)], cwd=root, env=environment, creationflags=flags, close_fds=True,
                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
