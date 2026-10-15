"""Deterministic distribution inventory, disposable DMG staging and the release version."""
import hashlib,json,os,pathlib,re,shutil,tempfile

# Same rule as crates/yoto-updater/src/version.rs: X.Y.Z, no leading zeros, no suffix.
RELEASE_VERSION=re.compile(r'(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)')
NIGHTLY='nightly'

def is_release_version(text):
    return RELEASE_VERSION.fullmatch(text) is not None

def app_version():
    """The release version given to this build in YOTO_APP_VERSION, or None for a nightly."""
    value=os.environ.get('YOTO_APP_VERSION','')
    if value=='':return None
    if not is_release_version(value):raise SystemExit('YOTO_APP_VERSION must be a release number such as 1.2.3, got '+repr(value))
    return value

def manifest(root,target):
    root=pathlib.Path(root)
    files={};links={}
    for file in sorted(root.rglob('*')):
        # Forward slashes on every platform: the app looks helpers up as youtube/yt-dlp.exe.
        relative=file.relative_to(root).as_posix()
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
