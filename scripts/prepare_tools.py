#!/usr/bin/env python3
"""Fetch verified upstream sidecars, build LGPL FFmpeg on macOS, retain sources."""
import argparse,hashlib,json,os,pathlib,platform,shutil,subprocess,tarfile,urllib.request,zipfile
from distribution_utils import write_manifest
ROOT=pathlib.Path(__file__).resolve().parents[1]
def run(args,**kw): subprocess.run([str(a) for a in args],check=True,**kw)
def fetch(item,cache):
    file=cache/item.get('filename',item['url'].rsplit('/',1)[1])
    if not file.exists():
        with urllib.request.urlopen(item['url'],timeout=120) as r,open(file.with_suffix(file.suffix+'.part'),'wb') as out:shutil.copyfileobj(r,out)
        file.with_suffix(file.suffix+'.part').replace(file)
    if hashlib.sha256(file.read_bytes()).hexdigest()!=item['sha256']:raise SystemExit('Checksum mismatch: '+file.name)
    return file
p=argparse.ArgumentParser();p.add_argument('--target',required=True,choices=['aarch64-apple-darwin','x86_64-apple-darwin','x86_64-pc-windows-msvc']);p.add_argument('--output',type=pathlib.Path);a=p.parse_args()
m=json.loads((ROOT/'distribution/sidecars.json').read_text());out=a.output or ROOT/'tools'/a.target
if out.exists():shutil.rmtree(out)
out.mkdir(parents=True)
cache=ROOT/'tools'/'downloads';cache.mkdir(parents=True,exist_ok=True);sources=out/'sources';sources.mkdir(exist_ok=True)
for name in ['yt-dlp','deno']:
    # The executables ship extracted (and, on macOS, signed); a copy of the downloaded
    # archive would hide unsigned code from codesign and fail notarization.
    item=m[name]['targets'][a.target];f=fetch(item,cache)
    if name=='deno':
        with zipfile.ZipFile(f) as z:z.extract('deno.exe' if 'windows' in a.target else 'deno',out)
    else:
        with zipfile.ZipFile(f) as z:z.extractall(out/'youtube')
        if 'windows' not in a.target:os.chmod(out/'youtube/yt-dlp_macos',0o755)
    if 'windows' not in a.target and name=='deno':os.chmod(out/name,0o755)
if 'windows' in a.target:
    f=fetch(m['ffmpeg']['targets'][a.target],cache);shutil.copy2(f,sources/f.name)
    for item in m['ffmpeg']['targets'][a.target]['sources']:
        source=fetch(item,cache);shutil.copy2(source,sources/source.name)
    with zipfile.ZipFile(f) as z:
        for n in z.namelist():
            base=pathlib.PurePosixPath(n).name
            if '/bin/' in n and (base in ['ffmpeg.exe','ffprobe.exe'] or base.endswith('.dll')) or base=='LICENSE.txt':
                with z.open(n) as r,open(out/base,'wb') as w:shutil.copyfileobj(r,w)
else:
    if platform.system()!='Darwin' or (platform.machine()=='arm64')!=(a.target.startswith('aarch64')):raise SystemExit('Build on the matching native macOS runner')
    build=ROOT/'tools'/'build'/a.target;build.mkdir(parents=True,exist_ok=True);prefix=build/'prefix'
    env=os.environ.copy();env['MACOSX_DEPLOYMENT_TARGET']='14.0'
    for name in ['lame','ffmpeg']:
        item=m[name]['source'];f=fetch(item,cache);shutil.copy2(f,sources/f.name)
        with tarfile.open(f) as tar:tar.extractall(build,filter='data')
    lame=build/'lame-3.100';sym=lame/'include/libmp3lame.sym';sym.write_text(sym.read_text().replace('lame_init_old\n',''))
    (sources/'lame-symbol.patch.txt').write_text('Remove obsolete lame_init_old export from include/libmp3lame.sym (Homebrew upstream fix https://sourceforge.net/p/lame/mailman/message/36081038/).\n')
    run(['./configure','--prefix='+str(prefix),'--disable-static','--enable-shared','--disable-frontend','--disable-dependency-tracking'],cwd=lame,env=env)
    run(['make','-j'+str(os.cpu_count())],cwd=lame,env=env);run(['make','install'],cwd=lame,env=env)
    ff=build/'ffmpeg-9.0.2'
    flags=['./configure','--prefix='+str(prefix),'--disable-autodetect','--disable-static','--enable-shared','--disable-doc','--disable-ffplay','--disable-network','--enable-libmp3lame','--extra-cflags=-I'+str(prefix/'include'),'--extra-ldflags=-L'+str(prefix/'lib')]
    run(flags,cwd=ff,env=env);run(['make','-j'+str(os.cpu_count())],cwd=ff,env=env);run(['make','install'],cwd=ff,env=env)
    for binary in ['ffmpeg','ffprobe']:shutil.copy2(prefix/'bin'/binary,out/binary)
    lib=out/'lib';lib.mkdir(exist_ok=True)
    for f in (prefix/'lib').glob('*.dylib'):
        destination=lib/f.name
        if destination.is_symlink() or destination.exists():destination.unlink()
        if f.is_symlink():destination.symlink_to(os.readlink(f))
        else:shutil.copy2(f,destination)
    for binary in [out/'ffmpeg',out/'ffprobe',*lib.glob('*.dylib')]:
        if binary.is_symlink():continue
        if binary.parent==lib:run(['install_name_tool','-id','@rpath/'+binary.name,binary])
        deps=subprocess.check_output(['otool','-L',str(binary)],text=True).splitlines()[1:]
        for line in deps:
            old=line.strip().split(' (')[0]
            if old.startswith(str(prefix)):
                replacement='@loader_path/'+('lib/' if binary.parent==out else '')+pathlib.Path(old).name
                run(['install_name_tool','-change',old,replacement,binary])
    for license in ['COPYING.LGPLv2.1','COPYING.LGPLv3']:shutil.copy2(ff/license,sources/license)
    shutil.copy2(lame/'COPYING',sources/'LAME-COPYING')
# Corresponding source and component notice are part of the distribution.
for name in ['yt-dlp','deno']:
    if 'source' in m[name]:f=fetch(m[name]['source'],cache);shutil.copy2(f,sources/f.name)
shutil.copy2(ROOT/'distribution/sidecars.json',out/'sidecars.json')
shutil.copy2(ROOT/'distribution/THIRD-PARTY-NOTICES.md',out/'THIRD-PARTY-NOTICES.md')
shutil.copy2(ROOT/'LICENSE',out/'LICENSE')
write_manifest(out,a.target)
