"""Deterministic distribution inventory and disposable DMG staging."""
import hashlib,json,pathlib,shutil,tempfile

def manifest(root,target):
    root=pathlib.Path(root)
    files={};links={}
    for file in sorted(root.rglob('*')):
        relative=str(file.relative_to(root))
        if relative=='manifest.json':continue
        if file.is_symlink():
            file.resolve(strict=True).relative_to(root.resolve())
            links[relative]=str(file.readlink())
        elif file.is_file():
            with file.open('rb') as stream: files[relative]=hashlib.file_digest(stream,'sha256').hexdigest()
    return {'target':target,'files':files,'links':links}

def write_manifest(root,target):
    (pathlib.Path(root)/'manifest.json').write_text(json.dumps(manifest(root,target),indent=2)+'\n')

def stage_dmg(app,parent):
    stage=tempfile.TemporaryDirectory(prefix='yoto-dmg-',dir=parent)
    root=pathlib.Path(stage.name)
    shutil.copytree(app,root/pathlib.Path(app).name,symlinks=True)
    (root/'Applications').symlink_to('/Applications')
    return stage
