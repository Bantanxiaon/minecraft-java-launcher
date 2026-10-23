"""Build a self-contained Windows x64 portable folder."""
from pathlib import Path
import os,json,shutil,subprocess

root=Path(__file__).resolve().parents[1]
settings=root/'.project/local-environment.json'
config=json.loads(settings.read_text(encoding='utf-8')) if settings.exists() else {}
dotnet=os.environ.get('SH_DOTNET') or config.get('dotnet') or shutil.which('dotnet')
if not dotnet:raise SystemExit('Install the SDK specified by global.json.')
env=os.environ.copy();env['DOTNET_ROOT']=str(Path(dotnet).resolve().parent)
env['DOTNET_CLI_TELEMETRY_OPTOUT']='1';env['DOTNET_GENERATE_ASPNET_CERTIFICATE']='false'
if config.get('cacheRoot'):
 for key,folder in [('DOTNET_CLI_HOME','cli'),('NUGET_PACKAGES','packages'),('NUGET_HTTP_CACHE_PATH','http'),('NUGET_PLUGINS_CACHE_PATH','plugins'),('XDG_CACHE_HOME','xdg')]:env[key]=str(Path(config['cacheRoot'])/folder)
if config.get('temp'):env['TEMP']=env['TMP']=config['temp']
output=root/'artifacts/SHLN-win-x64'
subprocess.run([dotnet,'publish',str(root/'src/SHLauncher.UI'),'-c','Release','-r','win-x64','--self-contained','true','-p:PublishSingleFile=false','-p:PublishTrimmed=false','-o',str(output)],cwd=root,env=env,check=True)
(output/'portable.flag').write_text('SHLN portable data directory\n',encoding='utf-8')
for name in ['LICENSE','THIRD-PARTY-NOTICES.md']:shutil.copy2(root/name,output/name)
shutil.copytree(root/'licenses',output/'licenses',dirs_exist_ok=True)
shutil.copy2(root/'docs/USAGE.md',output/'USAGE.md')
print(output)
