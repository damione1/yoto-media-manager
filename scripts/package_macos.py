#!/usr/bin/env python3
import argparse,hashlib,json,pathlib,plistlib,shutil,subprocess
from distribution_utils import write_manifest,stage_dmg
ROOT=pathlib.Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--target',required=True);p.add_argument('--identity',required=True);p.add_argument('--dev',action='store_true');p.add_argument('--notarize-profile');p.add_argument('--output-dir',type=pathlib.Path,help='Directory for this build, leaving another running bundle untouched');a=p.parse_args()
if a.identity in ['', '-']:raise SystemExit('A Developer ID signing identity is required')
if not a.dev and not a.notarize_profile:raise SystemExit('Release requires a notarization keychain profile')
name='Yoto Dev' if a.dev else 'Yoto';out_dir=a.output_dir if a.output_dir is not None else ROOT/'dist'/a.target;out=out_dir/(name+'.app');out.parent.mkdir(parents=True,exist_ok=True)
if out.exists():shutil.rmtree(out)
mac=out/'Contents/MacOS';resources=out/'Contents/Resources';mac.mkdir(parents=True);resources.mkdir()
profile='debug' if a.dev else 'release';build=ROOT/'target'/a.target/profile
if a.dev and not build.exists():build=ROOT/'target/debug'
for binary in ['yoto-desktop','yoto-mcp']:shutil.copy2(build/binary,mac/binary)
shutil.copytree(ROOT/'tools'/a.target,resources/'tools',symlinks=True)
shutil.copy2(ROOT/'assets/branding/app-icon.icns',resources/'app-icon.icns')
info={'CFBundleName':name,'CFBundleDisplayName':name,'CFBundleIdentifier':'dev.yoto.desktop'+('.dev' if a.dev else ''),'CFBundleExecutable':'yoto-desktop','CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.1.0','CFBundleVersion':'1','CFBundleIconFile':'app-icon.icns','LSMinimumSystemVersion':'14.0','NSHighResolutionCapable':True,'NSMicrophoneUsageDescription':'Audio output and local preview.'}
with open(out/'Contents/Info.plist','wb') as f:plistlib.dump(info,f)
def run(args):subprocess.run([str(v) for v in args],check=True)
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
    archive=out.parent/'notarize.zip';run(['ditto','-c','-k','--keepParent',out,archive]);run(['xcrun','notarytool','submit',archive,'--keychain-profile',a.notarize_profile,'--wait']);run(['xcrun','stapler','staple',out]);run(['xcrun','stapler','validate',out]);run(['spctl','--assess','--type','execute','--verbose=4',out])
    staging=stage_dmg(out,out.parent);stage=pathlib.Path(staging.name)
    dmg=out.parent/f'Yoto-0.1.0-{a.target}.dmg';run(['hdiutil','create','-volname','Yoto','-srcfolder',stage,'-ov','-format','UDZO',dmg]);run(['codesign','--timestamp','--sign',a.identity,dmg]);run(['xcrun','notarytool','submit',dmg,'--keychain-profile',a.notarize_profile,'--wait']);run(['xcrun','stapler','staple',dmg]);run(['xcrun','stapler','validate',dmg]);staging.cleanup()
print(out)
