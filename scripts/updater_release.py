#!/usr/bin/env python3
"""Build signed update archives or assemble a manifest from platform metadata.

The dedicated private key is accepted only from the process environment and
written to a private temporary file, never printed or retained in artifacts.
"""
import argparse, base64, json, os, pathlib, subprocess, sys, tarfile, tempfile, zipfile
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
from distribution_utils import app_version
ROOT = pathlib.Path(__file__).resolve().parents[1]
REPOSITORY = 'damione1/yoto-media-manager'
# Application and archive name; installed clients also accept the former 'Yoto' name.
APP_NAME = 'YotoMediaManager'
PLATFORMS = {'aarch64-apple-darwin': ('darwin-aarch64', 'tar.gz'), 'x86_64-apple-darwin': ('darwin-x86_64', 'tar.gz'), 'x86_64-pc-windows-msvc': ('windows-x86_64', 'zip')}
def version():
    current = app_version()
    if current is None: raise SystemExit('Update archives require YOTO_APP_VERSION from the release tag')
    return current
def build(target):
    platform, extension = PLATFORMS[target]
    current = version()
    output = ROOT / 'dist' / target
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f'{APP_NAME}-{current}-{platform}.{extension}'
    if extension == 'tar.gz':
        application = output / f'{APP_NAME}.app'
        if not application.is_dir(): raise SystemExit(f'Missing signed, notarized {APP_NAME}.app')
        with tarfile.open(archive, 'w:gz', dereference=False) as writer:
            writer.add(application, arcname=application.name)
    else:
        with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED, compresslevel=6) as writer:
            binaries = ROOT / 'target' / target / 'release'
            for name in ['yoto-desktop.exe', 'yoto-mcp.exe']:
                writer.write(binaries / name, f'{APP_NAME}/{name}')
            tools = ROOT / 'tools' / target
            for path in sorted(tools.rglob('*')):
                if path.is_file(): writer.write(path, f'{APP_NAME}/tools/' + path.relative_to(tools).as_posix())
            writer.write(ROOT / 'distribution' / 'THIRD-PARTY-NOTICES.md', f'{APP_NAME}/THIRD-PARTY-NOTICES.md')
    private_b64 = os.environ.get('YOTO_UPDATER_PRIVATE_KEY')
    if not private_b64: raise SystemExit('Missing required secret: YOTO_UPDATER_PRIVATE_KEY')
    signature = archive.with_name(archive.name + '.minisig')
    with tempfile.TemporaryDirectory(prefix='yoto-updater-signing-') as temporary:
        key = pathlib.Path(temporary) / 'private.key'
        descriptor = os.open(key, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600)
        with os.fdopen(descriptor, 'wb') as writer: writer.write(base64.b64decode(private_b64, validate=True))
        subprocess.run(['minisign', '-S', '-s', str(key), '-m', str(archive), '-x', str(signature), '-t', 'YotoMediaManager ' + archive.name], check=True)
        subprocess.run(['minisign', '-V', '-p', str(ROOT / 'distribution/updater.pub'), '-m', str(archive), '-x', str(signature)], check=True)
    metadata = {'version': current, 'platform': platform, 'url': f'https://github.com/{REPOSITORY}/releases/download/v{current}/{archive.name}', 'signature': base64.b64encode(signature.read_bytes()).decode('ascii'), 'size': archive.stat().st_size}
    (output / (platform + '.json')).write_text(json.dumps(metadata, indent=2) + '\n')
def assemble(folder):
    current = version()
    platforms = {}
    for platform, extension in PLATFORMS.values():
        metadata = json.loads((folder / (platform + '.json')).read_text())
        if metadata.pop('version') != current or metadata.pop('platform') != platform: raise SystemExit('Mismatched release metadata')
        filename = f'{APP_NAME}-{current}-{platform}.{extension}'
        archive = folder / filename
        if metadata['url'] != f'https://github.com/{REPOSITORY}/releases/download/v{current}/{filename}' or metadata['size'] != archive.stat().st_size: raise SystemExit('Mismatched release artifact')
        signature = archive.with_name(archive.name + '.minisig')
        if base64.b64encode(signature.read_bytes()).decode('ascii') != metadata['signature']: raise SystemExit('Mismatched release signature')
        subprocess.run(['minisign', '-V', '-p', str(ROOT / 'distribution/updater.pub'), '-m', str(archive), '-x', str(signature)], check=True)
        trusted = signature.read_text().splitlines()[2]
        if trusted != 'trusted comment: YotoMediaManager ' + filename: raise SystemExit('Mismatched signed release identity')
        platforms[platform] = metadata
    (folder / 'latest.json').write_text(json.dumps({'version': current, 'notes': 'Discover what is new in the release notes on GitHub.', 'platforms': platforms}, indent=2) + '\n')
if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    choice = parser.add_mutually_exclusive_group(required=True)
    choice.add_argument('--target', choices=PLATFORMS)
    choice.add_argument('--assemble', type=pathlib.Path)
    args = parser.parse_args()
    build(args.target) if args.target else assemble(args.assemble)
