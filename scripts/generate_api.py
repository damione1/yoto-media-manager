#!/usr/bin/env python3
"""Deterministic, offline normalization and generation from the official snapshot."""
import copy, hashlib, json, pathlib, subprocess, os
from openapi_examples import assert_safe_examples
ROOT = pathlib.Path(__file__).resolve().parents[1]
up = ROOT/'api/upstream/yoto.openapi.json'
s = json.loads(up.read_text())
O = ROOT/'api/overlays/contract.json'
overlay = json.loads(O.read_text())
assert_safe_examples(s)
assert_safe_examples(overlay)
jar=ROOT/'tools/openapi-generator-cli-7.26.0.jar'
jar.parent.mkdir(exist_ok=True)
if not jar.exists():
 import urllib.request
 urllib.request.urlretrieve('https://repo.maven.apache.org/maven2/org/openapitools/openapi-generator-cli/7.26.0/openapi-generator-cli-7.26.0.jar',jar)
expected=(ROOT/'api/generator.sha256').read_text().strip()
if hashlib.sha256(jar.read_bytes()).hexdigest()!=expected:raise SystemExit('Generator checksum mismatch')
for p,v in s['paths'].items():
 v.pop('servers',None)
 for method,op in v.items():
  if method not in ['get','post','put','delete','patch']:continue
  if p.startswith('/oauth') or p == '/authorize':op['security']=[]
for p in ['/media/displayIcons/user/me/upload','/media/coverImage/user/me/upload']:
 s['paths'][p]['post']['requestBody']={'required':True,'content':{'application/octet-stream':{'schema':{'type':'string','format':'binary'}}}}
old=next(p for p in s['paths'] if p.startswith('/media/family/images/') and len(p.split('/'))==5)
s['paths']['/media/family/images/{imageId}']=s['paths'].pop(old)
s['paths']['/media/family/images/{imageId}']['get']['parameters']=[{'name':'imageId','in':'path','required':True,'schema':{'type':'string'}}]
# Preserve official inline DTOs; named enriched DTOs come from the versioned overlay.
s['paths']['/media/family/images']['post']['requestBody']={'required':True,'content':{'application/octet-stream':{'schema':{'type':'string','format':'binary'}}}}
s['components'].setdefault('schemas',{}).update(overlay['schemas'])
for path, methods in overlay['paths'].items():
 s['paths'].setdefault(path, {}).update(methods)
for path, item in s['paths'].items():
 item.pop('servers', None)
 for method, operation in item.items():
  if method in ['get','post','put','delete','patch']:
   operation.pop('servers', None)
   if path.startswith('/oauth') or path == '/authorize':operation['security']=[]
for path in ['/media/displayIcons/user/me/upload','/media/coverImage/user/me/upload']:
 s['paths'][path]['post']['requestBody']={'required':True,'content':{'application/octet-stream':{'schema':{'type':'string','format':'binary'}}}}
s['paths']['/media/family/images']['get']['responses']['200']['content']['application/json']['schema']={'$ref':'#/components/schemas/FamilyImagesResponse'}
s['paths']['/media/family/images']['post']['responses']['200']['content']['application/json']['schema']={'$ref':'#/components/schemas/FamilyImageUploaded'}
s['paths']['/content']['post']['requestBody']['content']['application/json']['schema']={'$ref':'#/components/schemas/Card'}
s['paths']['/content/{cardId}']['get']['responses']['200']['content']['application/json']['schema']={'$ref':'#/components/schemas/CardResponse'}
def preserve_extensions(value):
 if isinstance(value, dict):
  if value.get('type') == 'object' and value.get('properties') and value.get('additionalProperties') is not False:
   value['x-preserve-extra'] = True
  vocabulary={'autoadvance':'Autoadvance','playbackType':'PlaybackType','category':'Category','channels':'AudioChannels','format':'AudioFormat'}
  for field, schema in value.get('properties', {}).items():
   if field in vocabulary and schema.get('type') == 'string':
    value['properties'][field]={'$ref':'#/components/schemas/'+vocabulary[field]}
  for child in value.values():preserve_extensions(child)
 elif isinstance(value, list):
  for child in value:preserve_extensions(child)
preserve_extensions(s)
for kind in ['resources','auth']:
 out=copy.deepcopy(s)
 out['paths']={p:v for p,v in out['paths'].items() if (p.startswith('/oauth') or p == '/authorize') == (kind=='auth')}
 out['servers']=[{'url':'https://login.yotoplay.com' if kind=='auth' else 'https://api.yotoplay.com'}]
 if kind=='auth':out['security']=[]
 dest=ROOT/f'api/effective/{kind}.openapi.json'
 dest.write_text(json.dumps(out,indent=2,sort_keys=True)+'\n')
 package='yoto-auth-generated' if kind=='auth' else 'yoto-api-generated'
 import shutil
 output=ROOT/'crates'/package
 if output.exists():shutil.rmtree(output)
 subprocess.run(['java','-jar',str(jar),'generate','-t',str(ROOT/'api/templates'),'-g','rust','--library','reqwest','-i',str(dest),'-o',str(ROOT/'crates'/package),'--schema-mappings',','.join(n+'=yoto_contracts::'+n for n,sch in overlay['schemas'].items() if 'enum' in sch),'--additional-properties',f'packageName={package},packageVersion=0.1.0,hideGenerationTimestamp=true'],check=True,stdout=subprocess.DEVNULL)
 mods=ROOT/'crates'/package/'src/models/mod.rs'
 mods.write_text(mods.read_text()+'\npub use yoto_contracts::{'+','.join(n for n,sch in overlay['schemas'].items() if 'enum' in sch)+'};\n')
 # These wire structures are also the persisted/editor contract: generate once and re-export.
 for model in ['event','track_events']:
  source=output/'src/models'/f'{model}.rs'
  if kind=='resources':
   text=source.read_text().replace('use crate::models;','use crate as models;\nuse schemars::JsonSchema;').replace('PartialEq, Serialize, Deserialize)]','PartialEq, Serialize, Deserialize, JsonSchema)]')
   (ROOT/'crates/yoto-contracts/src'/f'{model}.rs').write_text(text)
  source.unlink()
  text=mods.read_text()
  text=text.replace(f'pub mod {model};\n','').replace(f'pub use self::{model}::'+('Event' if model=='event' else 'TrackEvents')+';\n','')
  mods.write_text(text)
 mods.write_text(mods.read_text()+'\npub use yoto_contracts::{Event, TrackEvents};\n')
 manifest=ROOT/'crates'/package/'Cargo.toml'
 manifest.write_text(manifest.read_text().replace('[dependencies]', '[dependencies]\nyoto-contracts = { path = \"../yoto-contracts\" }'))
 for path in ['git_push.sh','.travis.yml','.github','.openapi-generator-ignore','docs','README.md']:
  q=ROOT/'crates'/package/path
  if q.is_dir():import shutil;shutil.rmtree(q)
  elif q.exists():q.unlink()
# Leaf vocabulary is generated once; DTO conversion consumes this same vocabulary.
import tomllib
public=tomllib.loads((ROOT/'config/yoto.toml').read_text())
scopes=public['oauth']['requested_scopes']+['family:device-status:view']
overlay['schemas']['Scope']={'type':'string','enum':scopes}
lines=['// Generated by scripts/generate_api.py; do not edit.','use serde::{Serialize, Deserialize};','use schemars::JsonSchema;']
for name,sch in overlay['schemas'].items():
 if 'enum' not in sch:continue
 vals=sch['enum'];variants=[ ''.join(w.title() for w in v.replace('-','_').replace(':','_').split('_')) or 'Empty' for v in vals]
 lines += ['#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]',f'pub enum {name} {{']
 for v,var in zip(vals,variants):lines += [f'    #[serde(rename = {json.dumps(v)})] {var},']
 lines += ['}', f'impl {name} {{',f'    pub const ALL: &[Self] = &[{", ".join("Self::"+v for v in variants)}];','    pub fn wire(self) -> &\'static str { match self {']
 for v,var in zip(vals,variants):lines += [f'        Self::{var} => {json.dumps(v)},']
 lines+=['    }}','}']
lines += ['pub mod event;', 'pub mod track_events;', 'pub use event::Event;', 'pub use track_events::TrackEvents;']
(ROOT/'crates/yoto-contracts/src/lib.rs').write_text('\n'.join(lines)+'\n')
inventory=[]
for p,v in s['paths'].items():
 for m,o in v.items():
  if m in ['get','post','put','delete','patch']:inventory.append({'method':m,'path':p,'operation':o['operationId']})
policy=json.loads((ROOT/'api/coverage-policy.json').read_text())
operations={item['operation'] for item in inventory}
if operations != set(policy):raise SystemExit('Coverage policy must classify every effective OpenAPI operation exactly once')
for item in inventory:item.update(policy[item['operation']])
(ROOT/'api/coverage.json').write_text(json.dumps(inventory,indent=2,ensure_ascii=False)+'\n')
report=['# Couverture des opérations Yoto', '', 'Généré depuis le contrat effectif et `api/coverage-policy.json`. Toutes les commandes citées sont découvrables dans le schéma Rust de `yoto_execute`; `yoto_snapshot` lit catalogue, tâches et associations. « implemented » signifie raccordé, pas validé sur un compte réel.', '', '| Opération | Service / commande | UI | Statut | Preuve ou gate |', '| --- | --- | --- | --- | --- |']
for item in inventory:
 report.append('| '+item['method'].upper()+' `'+item['path']+'` | '+item['service']+' / '+', '.join(item['commands'])+' | '+item['ui']+' | '+item['status']+' | '+item['evidence_or_gate']+' |')
report += ['', 'Les commandes locales (import/tri/sélection, inspection YouTube, recettes et suppression avec références/GC, waveform/zoom/seek/playhead, preview/stop, export original, backup/restore complet, édition/duplication/suppression/remplacement audio de playlist, pixelart, comptes, jobs paginés/retry/cancel/résolution) ne correspondent pas à un endpoint Yoto. Elles sont exposées par la même union `Command`, sans dispatcher métier séparé dans MCP.', '']
(ROOT/'docs/api-coverage.md').write_text('\n'.join(report))

subprocess.run(['cargo','fmt','-p','yoto-contracts','-p','yoto-api-generated','-p','yoto-auth-generated'],cwd=ROOT,check=True)
