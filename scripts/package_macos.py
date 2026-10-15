#!/usr/bin/env python3
import argparse,json,pathlib,plistlib,shutil,subprocess
from distribution_utils import app_version,write_manifest,stage_dmg
ROOT=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--target',required=True);p.add_argument('--identity',required=True);p.add_argument('--dev',action='store_true');p.add_argument('--notarize-profile');p.add_argument('--output-dir',type=pathlib.Path,help='Directory for this build, leaving another running bundle untouched');a=p.parse_args()
if a.identity in ['', '-']:raise SystemExit('A Developer ID signing identity is required')
if not a.dev and not a.notarize_profile:raise SystemExit('Release requires a notarization keychain profile')
# The tag's version for a release; a nightly bundle still needs numeric Info.plist versions.
version=app_version()
if version is None and not a.dev:raise SystemExit('A release bundle requires YOTO_APP_VERSION from the release tag')
bundle_version=version or '0.0.0'
# The bundle identifier stays dev.yoto.desktop(.dev): keychain items, permissions and the
# updater's signing check are tied to it, so renaming the bundle must not change it.
name='YotoMediaManager Dev' if a.dev else 'YotoMediaManager';out_dir=a.output_dir if a.output_dir is not None else ROOT/'dist'/a.target;out=out_dir/(name+'.app');out.parent.mkdir(parents=True,exist_ok=True)
if out.exists():shutil.rmtree(out)
mac=out/'Contents/MacOS';resources=out/'Contents/Resources';mac.mkdir(parents=True);resources.mkdir()
profile='debug' if a.dev else 'release';build=ROOT/'target'/a.target/profile
if a.dev and not build.exists():build=ROOT/'target/debug'
for binary in ['yoto-desktop','yoto-mcp']:shutil.copy2(build/binary,mac/binary)
shutil.copytree(ROOT/'tools'/a.target,resources/'tools',symlinks=True)
shutil.copy2(ROOT/'assets/branding/app-icon.icns',resources/'app-icon.icns')
info={'CFBundleName':name,'CFBundleDisplayName':name,'CFBundleIdentifier':'dev.yoto.desktop'+('.dev' if a.dev else ''),'CFBundleExecutable':'yoto-desktop','CFBundlePackageType':'APPL','CFBundleShortVersionString':bundle_version,'CFBundleVersion':bundle_version,'CFBundleIconFile':'app-icon.icns','LSMinimumSystemVersion':'14.0','NSHighResolutionCapable':True,'NSMicrophoneUsageDescription':'Audio output and local preview.'}
with open(out/'Contents/Info.plist','wb') as f:plistlib.dump(info,f)
def run(args):subprocess.run([str(v) for v in args],check=True)
def notarize(path):
    # Print Apple's own log when a submission is refused, so a failed release says why.
    result=subprocess.run(['xcrun','notarytool','submit',str(path),'--keychain-profile',a.notarize_profile,'--wait','--output-format','json'],capture_output=True,text=True);print(result.stdout,result.stderr)
    submission=json.loads(result.stdout or '{}')
    if result.returncode or submission.get('status')!='Accepted':
        if submission.get('id'):run(['xcrun','notarytool','log',submission['id'],'--keychain-profile',a.notarize_profile])
        raise SystemExit('Notarization was not accepted')
# The upstream ZIP flattened framework symlinks into copies. Restore the canonical
# layout before signing; otherwise Apple detects an ambiguous app/framework bundle.
for framework in out.rglob('Python.framework'):
    current=framework/'Versions/Current';shutil.rmtree(current);current.symlink_to('3.14')
    for name in ['Python','Resources']:
        alias=framework/name
        if alias.is_dir():shutil.rmtree(alias)
        elif alias.exists():alias.unlink()
        alias.symlink_to('Versions/Current/'+name)
magic={b'\xfe\xed\xfa\xce',b'\xce\xfa\xed\xfe',b'\xfe\xed\xfa\xcf',b'\xcf\xfa\xed\xfe',b'\xca\xfe\xba\xbe',b'\xbe\xba\xfe\xca',b'\xca\xfe\xba\xbf',b'\xbf\xba\xfe\xca'}
# Sign each embedded Mach-O (.so, Python, framework copies and dylibs included), innermost first.
for f in sorted(out.rglob('*'),key=lambda p:len(p.parts),reverse=True):
    if f.is_file() and not f.is_symlink():
        with open(f,'rb') as r:header=r.read(4)
        if header in magic:
            args=['codesign','--force','--timestamp','--options','runtime','--sign',a.identity]
            if f.name=='deno':args+=['--entitlements',ROOT/'distribution/deno.entitlements.plist']
            run(args+[f])
for f in sorted(out.rglob('*.framework'),key=lambda p:len(p.parts),reverse=True):run(['codesign','--force','--timestamp','--options','runtime','--sign',a.identity,f])
# Regenerate the sidecar manifest after codesign changes bytes.
tools=resources/'tools';write_manifest(tools,a.target)
run(['codesign','--force','--timestamp','--options','runtime','--sign',a.identity,out]);run(['codesign','--verify','--deep','--strict','--verbose=2',out])
run([mac/'yoto-mcp','--self-test'])
if not a.dev:
    archive=out.parent/'notarize.zip';run(['ditto','-c','-k','--keepParent',out,archive]);notarize(archive);run(['xcrun','stapler','staple',out]);run(['xcrun','stapler','validate',out]);run(['spctl','--assess','--type','execute','--verbose=4',out])
    staging=stage_dmg(out,out.parent);stage=pathlib.Path(staging.name)
    dmg=out.parent/f'YotoMediaManager-{version}-{a.target}.dmg';run(['hdiutil','create','-volname','YotoMediaManager','-srcfolder',stage,'-ov','-format','UDZO',dmg]);run(['codesign','--timestamp','--sign',a.identity,dmg]);notarize(dmg);run(['xcrun','stapler','staple',dmg]);run(['xcrun','stapler','validate',dmg]);staging.cleanup()
print(out)
