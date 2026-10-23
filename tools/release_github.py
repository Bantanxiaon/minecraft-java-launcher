"""Package the built portable folder and publish this version on GitHub."""
from pathlib import Path
import hashlib,os,subprocess,xml.etree.ElementTree as ET,zipfile

root=Path(__file__).resolve().parents[1]
version=ET.parse(root/'Directory.Build.props').getroot().findtext('.//Version')
if not version or any(c not in '0123456789.-abcdefghijklmnopqrstuvwxyz' for c in version):raise SystemExit('Invalid version')
repository=os.environ.get('GITHUB_REPOSITORY')
if not repository or not os.environ.get('GH_TOKEN'):raise SystemExit('Run this from the repository release workflow.')
folder=root/'artifacts/SHLN-win-x64';archive=root/'artifacts'/f'SHLN-{version}-windows-x64.zip'
if not (folder/'SHLauncher.UI.exe').is_file() or not (folder/'coreclr.dll').is_file():raise SystemExit('Build the self-contained package first.')
with zipfile.ZipFile(archive,'w',zipfile.ZIP_DEFLATED,compresslevel=6) as z:
    for path in sorted(folder.rglob('*')):
        relative=path.relative_to(folder)
        if path.is_file() and 'data' not in relative.parts and path.suffix not in {'.pdb','.log'}:z.write(path,Path('SHLN')/relative)
with zipfile.ZipFile(archive) as z:
    if z.testzip() is not None:raise SystemExit('Archive integrity check failed.')
checksums=archive.parent/'SHA256SUMS.txt'
checksums.write_text(hashlib.sha256(archive.read_bytes()).hexdigest()+'  '+archive.name+'\n',encoding='utf-8')
tag='v'+version
existing=subprocess.run(['gh','release','view',tag,'--repo',repository],capture_output=True)
if existing.returncode==0:raise SystemExit('This version already has a release; increment Version before publishing again.')
subprocess.run(['gh','release','create',tag,str(archive),str(checksums),'--repo',repository,'--target',os.environ['GITHUB_SHA'],'--title','SHLN '+version,'--notes-file',str(root/'docs/RELEASE.md'),'--latest'],cwd=root,check=True)
