"""Run the isolated presentation prototype independently of any editor or chat process."""
import json
import os
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parents[1]
config_file = root / ".project" / "local-environment.json"
config = json.loads(config_file.read_text(encoding="utf-8")) if config_file.exists() else {}
dotnet = os.environ.get("SH_DOTNET") or config.get("dotnet") or shutil.which("dotnet")
application = root / "tools" / "ShellProbe" / "bin" / "Release" / "net10.0" / "ShellProbe.dll"
if not dotnet or not application.is_file():
    raise SystemExit("Build tools/ShellProbe/ShellProbe.csproj in Release first.")
environment = os.environ.copy()
environment["DOTNET_ROOT"] = str(Path(dotnet).resolve().parent)
environment["SHLN_SHELL_GATE_EVIDENCE"] = str(root / ".project" / "ui-rebuild" / "native-checks")
if config.get("temp"):
    environment["TEMP"] = environment["TMP"] = config["temp"]
# This prototype has no account/provider integration.
for name in ("SH_CURSEFORGE_API_KEY", "SH_CURSEFORGE_CONFIG_FILE"):
    environment.pop(name, None)
flags = (subprocess.CREATE_NEW_PROCESS_GROUP | subprocess.DETACHED_PROCESS) if os.name == "nt" else 0
process = subprocess.Popen([dotnet, str(application), "InApp"], cwd=root, env=environment,
                 creationflags=flags, close_fds=True,
                 stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

evidence = root / ".project" / "ui-rebuild" / "native-checks"
evidence.mkdir(parents=True, exist_ok=True)
(evidence / "process.json").write_text(json.dumps({"Pid": process.pid}), encoding="utf-8")
