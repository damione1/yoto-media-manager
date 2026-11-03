import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {runInNewContext} from 'node:vm';
import {securityHeaders} from '../security-headers.js';
import {parseRelease, downloadTargets} from '../releases.js';
const release=parseRelease(JSON.parse(readFileSync(new URL('./fixtures/release.json',import.meta.url))));
const source=readFileSync(new URL('../src/js/analytics.js',import.meta.url),'utf8');
function browser({host='yotomediamanager.app',group='landing'}={}) {
  const listeners={}, scripts=[]; let now=0; let interval;
  const document={currentScript:{dataset:{measurementId:'G-TEST123',pageLocale:'fr',contentGroup:group,contentId:'/docs/overview/'}},
    visibilityState:'visible',head:{append:s=>scripts.push(s)},createElement:()=>({}),
    querySelector:()=>({getBoundingClientRect:()=>({top:0,height:1000})}),
    addEventListener:(name,fn)=>listeners[name]=fn};
  const window={addEventListener:(name,fn)=>listeners[name]=fn};
  runInNewContext(source,{window,document,location:new URL('https://'+host+'/fr/docs/overview/'),
    sessionStorage:{getItem:()=>null},URL,Date,performance:{now:()=>now},innerHeight:800,
    setInterval:fn=>(interval=fn,1),clearInterval:()=>{}});
  return {window,document,scripts,listeners,tick:ms=>{now+=ms;interval?.();},
    events:()=>Array.from(window.dataLayer||[],args=>Array.from(args)).filter(args=>args[0]==='event')};
}
function link(url,dataset={}) {return {href:url,dataset,closest:()=>null};}
function click(b,a,button=0) {b.listeners.click({type:'click',button,target:{closest:()=>a}});}

test('one canonical bootstrap, precise download variants, and no arbitrary payloads',()=>{
  const b=browser();
  assert.equal(b.scripts.length,1);
  assert.equal(Array.from(b.window.dataLayer).filter(a=>a[0]==='config').length,1);
  for(const d of Object.values(release.downloads)) click(b,link(d.url,{downloadVersion:release.version,
    downloadPlatform:d.platform,downloadArchitecture:d.architecture,releaseChannel:d.channel}));
  assert.deepEqual(b.events().map(e=>[e[1],e[2].app_version,e[2].download_platform,e[2].download_architecture]),
    [['app_download',release.version,'macos','arm64'],['app_download',release.version,'macos','x86_64'],['app_download',release.version,'windows','x86_64']]);
  click(b,link('https://github.com/damione1/yoto-media-manager/releases'));
  assert.equal(b.events().at(-1)[1],'release_page_open');
  const before=b.events().length;
  click(b,link(Object.values(release.downloads)[0].url),2);
  assert.equal(b.events().length,before);
  b.window.siteAnalytics.track('app_download',{search_term:'private',clipboard:'private',value:-1});
  assert.equal(b.events().at(-1)[2].search_term,undefined);
  assert.equal(b.events().at(-1)[2].clipboard,undefined);
  assert.equal(b.events().at(-1)[2].value,undefined);
  const local=browser({host:'localhost'}); assert.equal(local.scripts.length,0);
  local.window.siteAnalytics.track('app_download'); assert.equal(local.events().length,0);
});

test('reading requires 30 seconds in the foreground and is emitted once',()=>{
  const b=browser({group:'documentation'});
  b.tick(20000); assert.equal(b.events().length,0);
  b.document.visibilityState='hidden'; b.listeners.visibilitychange();
  b.tick(60000); assert.equal(b.events().length,0);
  b.document.visibilityState='visible'; b.listeners.visibilitychange();
  b.tick(9999); assert.equal(b.events().length,0);
  b.tick(1); assert.equal(b.events()[0][1],'doc_read');
  assert.equal(b.events()[0][2].value,30);
  assert.equal(b.events()[0][2].content_id,'/docs/overview/');
  b.tick(60000); assert.equal(b.events().length,1);
});

test('built downloads agree with the release snapshot and strict CSP permits Analytics',()=>{
  for(const path of ['index.html','fr/index.html']) {
    const html=readFileSync(new URL('../_site/'+path,import.meta.url),'utf8');
    const version=html.match(/data-download-version="([^"]+)"/)[1];
    for(const target of Object.values(downloadTargets)) assert.ok(html.includes('/releases/download/v'+version+'/'+target.filename));
    const tags = [...html.matchAll(/data-measurement-id="([^"]+)"/g)].map(match => match[1]);
    assert.deepEqual(tags, process.env.GA_MEASUREMENT_ID ? [process.env.GA_MEASUREMENT_ID] : []);
    for(const match of html.matchAll(/<script([^>]*)>([\s\S]*?)<\/script>/g)) {
      assert.ok(match[1].includes('src=') || match[1].includes('application/ld+json'));
    }
  }
  const csp=securityHeaders['Content-Security-Policy'];
  assert.ok(csp.includes("script-src 'self' https://www.googletagmanager.com"));
  assert.ok(csp.includes('https://*.google-analytics.com'));
  assert.ok(!csp.includes('unsafe-inline')); assert.ok(!csp.includes('unsafe-eval'));
});
