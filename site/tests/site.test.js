import test from "node:test";
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

const output = new URL("../_site/", import.meta.url).pathname;
const read = (path) => readFileSync(join(output, path), "utf8");
const fileFor = (path) => path.endsWith("/") ? `${path}index.html` : path;
const urls = [...read("sitemap.xml").matchAll(/<loc>([^<]+)<\/loc>/g)].map((m) => new URL(m[1]));
const pages = urls.map((url) => ({ url, html: read(fileFor(url.pathname)) }));
const attr = (tag, name) => tag.match(new RegExp(`\\b${name}="([^"]*)"`))?.[1];
const meta = (html, name) => attr([...html.matchAll(/<meta\b[^>]*>/g)].map((m) => m[0]).find((tag) => attr(tag, "name") === name || attr(tag, "property") === name) || "", "content");
const scripts = (html) => [...html.matchAll(/<script type="application\/ld\+json">([\s\S]*?)<\/script>/g)].map((m) => JSON.parse(m[1]));

test("all 22 indexable pages have unique descriptions, headings and reciprocal language links", () => {
  assert.equal(pages.length, 22);
  const descriptions = new Set();
  const titles = new Set();
  for (const { url, html } of pages) {
    const description = meta(html, "description");
    assert.ok(description?.length > 40, url.href);
    assert.equal(meta(html, "og:description"), description, url.href);
    assert.equal(meta(html, "twitter:description"), description, url.href);
    descriptions.add(description);
    titles.add(html.match(/<title>([^<]+)<\/title>/)[1]);
    assert.equal([...html.matchAll(/<h1\b/g)].length, 1, url.href);
    assert.notEqual(meta(html, "robots"), "noindex", url.href);
    assert.ok(html.includes(`<link rel="canonical" href="${url.href}">`), url.href);
    for (const match of html.matchAll(/<link rel="alternate"[^>]*>/g)) {
      const target = new URL(attr(match[0], "href"));
      const other = pages.find((p) => p.url.href === target.href);
      assert.ok(other, target.href);
      assert.ok(other.html.includes(`href="${url.href}"`), target.href);
    }
    for (const schema of scripts(html)) assert.ok(schema["@type"]);
  }
  assert.equal(descriptions.size, pages.length);
  assert.equal(titles.size, pages.length);
});

test("internal links, fragments and responsive images resolve to built files", () => {
  for (const { url, html } of pages) {
    for (const match of html.matchAll(/<(?:a|img|source)\b[^>]*>/g)) {
      const tag = match[0];
      const paths = [attr(tag, "href"), attr(tag, "src")].filter(Boolean);
      for (const src of (attr(tag, "srcset") || "").split(",").filter(Boolean)) paths.push(src.trim().split(/\s+/)[0]);
      for (const path of paths) {
        const target = new URL(path, url);
        if (target.origin !== url.origin) continue;
        const file = fileFor(target.pathname);
        assert.ok(existsSync(join(output, file)), `${url.pathname} → ${path}`);
        if (target.hash) assert.ok(read(file).includes(`id="${decodeURIComponent(target.hash.slice(1))}"`), `${url.pathname} → ${path}`);
      }
    }
  }
});

test("guide breadcrumbs describe the visible hierarchy and app facts stay truthful", () => {
  for (const { url, html } of pages) {
    if (url.pathname.includes("/docs/")) {
      const trail = scripts(html).find((s) => s["@type"] === "BreadcrumbList");
      assert.ok(trail, url.href);
      assert.equal(trail.itemListElement.at(-1).item, url.href);
      assert.ok(html.includes('class="breadcrumbs"'));
    } else {
      const app = scripts(html).find((s) => s["@type"] === "SoftwareApplication");
      assert.equal(app.offers.price, "0");
      assert.equal(app.downloadUrl.length, 3);
      assert.equal(app.aggregateRating, undefined);
      assert.equal(app.review, undefined);
      assert.ok(html.includes("GPL-3.0"));
    }
  }
});

test("normalization is permanent, loop-free and never replaces missing pages with a 200", () => {
  const redirects = new Map();
  for (const line of read("_redirects").split("\n").filter((l) => l && !l.startsWith("#"))) {
    const [source, target, status] = line.trim().split(/\s+/);
    assert.equal(status, "301");
    assert.notEqual(source, target);
    assert.ok(existsSync(join(output, fileFor(target))), target);
    redirects.set(source, target);
  }
  for (const [source, target] of redirects) assert.ok(!redirects.has(target), `${source} redirects twice`);
  for (const path of ["404.html", "fr/404.html"]) assert.equal(meta(read(path), "robots"), "noindex");
  const config = JSON.parse(readFileSync(new URL("../wrangler.jsonc", import.meta.url), "utf8"));
  assert.equal(config.assets.not_found_handling, "404-page");
  assert.equal(config.name, "yoto-media-manager");
  assert.equal(config.assets.run_worker_first, true);
  assert.equal(config.workers_dev, true);
  assert.deepEqual(config.routes.map((route) => [route.pattern, route.custom_domain]), [
    ["yotomediamanager.app", true],
    ["www.yotomediamanager.app", true],
    ["yotomediamanager.damiengoehrig.ca", true],
  ]);
});

test("static asset CSP uses literal policy syntax rather than HTML entities", () => {
  const headers = read("_headers");
  assert.ok(headers.includes("script-src 'self'"));
  assert.ok(headers.includes("object-src 'none'"));
  assert.ok(!headers.includes("&#"));
});
