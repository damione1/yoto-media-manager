/**
 * Two locales, one set of templates. The landing page renders once per entry
 * in `_data/locales.js` with its copy from `_data/en.json` / `_data/fr.json`;
 * the user guide is plain Markdown, one folder per language (`src/docs/`,
 * `src/fr/docs/`), so the same files read fine on GitHub.
 *
 * Output is static files in _site. wrangler.jsonc configures asset routing;
 * the HTML Worker only adds a fresh CSP nonce for Cloudflare's bot detection.
 */
import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import yotoIcons from "./src/_data/yotoIcons.js";
import { securityHeaders } from "./security-headers.js";

const root = dirname(fileURLToPath(import.meta.url));
const srcDir = join(root, "src");

/** Heading ids that survive accents: "Créer et modifier" → "creer-et-modifier". */
export function slugify(text) {
  return String(text)
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[’'`]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** Width and height straight from a PNG header, so guide images reserve
 *  their space before they load and never shift the text below them. */
function pngSize(file) {
  const buf = readFileSync(file);
  if (buf.toString("ascii", 1, 4) !== "PNG") return null;
  return { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20) };
}

export default function (eleventyConfig) {
  eleventyConfig.addGlobalData("securityHeaders", securityHeaders);
  for (const [from, to] of Object.entries({
    "src/css": "css",
    "src/js": "js",
    "src/fonts": "fonts",
    "src/img": "img",
    "src/docs/images": "docs/images",
    "src/favicon.ico": "favicon.ico",
    "src/apple-touch-icon.png": "apple-touch-icon.png",
    "src/icon-192.png": "icon-192.png",
    "src/icon-512.png": "icon-512.png",
    "src/og-en.png": "og-en.png",
    "src/og-fr.png": "og-fr.png",
    "src/site.webmanifest": "site.webmanifest",
  })) {
    eleventyConfig.addPassthroughCopy({ [from]: to });
  }

  // CSS and JS keep stable filenames and are cached for a year (see
  // _headers), so each build stamps a content hash onto their URL.
  eleventyConfig.addFilter("bust", (url) => {
    const digest = createHash("sha256").update(readFileSync(join(srcDir, url))).digest("hex");
    return `${url}?v=${digest.slice(0, 8)}`;
  });

  // Serializes a plain object to a JSON-LD-safe string.
  eleventyConfig.addFilter("jsonld", (obj) => JSON.stringify(obj).replace(/</g, "\\u003c"));

  // "{n} audios" style placeholders in the copy bundles.
  eleventyConfig.addFilter("fill", (str, vars) =>
    String(str).replace(/\{(\w+)\}/g, (m, k) => (vars && k in vars ? vars[k] : m)),
  );

  // The same page in the other language: "/docs/x/" ⇄ "/fr/docs/x/".
  eleventyConfig.addFilter("localeUrl", (url, code) => {
    const neutral = String(url).replace(/^\/fr(\/|$)/, "/");
    return code === "fr" ? `/fr${neutral}` : neutral;
  });

  // The trim dialog's waveform: quiet intro and credits, louder middle,
  // with each bar's class for the scene's finished frame (the selection
  // 0:16 → 1:39 of a 1 min 52 s song, the playhead clicked at 45 %):
  // outside the kept range, inside it, or before the playhead.
  eleventyConfig.addGlobalData("waveBars", () => {
    const n = 72, a = 16 / 112, b = 99 / 112, p = 0.45;
    return Array.from({ length: n }, (_, i) => {
      const x = (i + 0.5) / n;
      const env = x < 0.12 ? 0.25 : x > 0.9 ? 0.3 : 1;
      const h = Math.round(18 + env * (40 + 60 * Math.abs(Math.sin(i * 0.7) * Math.cos(i * 0.23))));
      const cls = x < a || x > b ? "out" : x <= p ? "played" : "in";
      // Twelve height steps, as classes: the CSP forbids style attributes.
      const level = Math.round(((Math.min(100, h) - 18) / 82) * 11);
      return { level, cls };
    });
  });

  // A 16 × 16 icon from _data/yotoIcons.js as inline SVG: one <rect> per
  // run of same-colour pixels, presentation attributes only (no style).
  eleventyConfig.addFilter("yicon", (name, cls = "") => {
    const runs = yotoIcons[name];
    if (!runs) throw new Error(`yicon: unknown icon "${name}"`);
    const rects = runs.map((r) => `<rect x="${r.x}" y="${r.y}" width="${r.w}" height="1" fill="${r.c}"/>`).join("");
    return `<svg class="yi${cls ? ` ${cls}` : ""}" viewBox="0 0 16 16" shape-rendering="crispEdges" aria-hidden="true" focusable="false">${rects}</svg>`;
  });

  // A guide page's title, split from its body, so "On this page" can sit
  // between them.
  eleventyConfig.addFilter("splitTitle", (html) => {
    const m = String(html).match(/^\s*(<h1[^>]*>[\s\S]*?<\/h1>)([\s\S]*)$/);
    return m ? [m[1], m[2]] : ["", String(html)];
  });

  // The "On this page" list: every h2 the Markdown renderer gave an id to.
  eleventyConfig.addFilter("toc", (html) => {
    const out = [];
    const re = /<h2 id="([^"]+)"[^>]*>([\s\S]*?)<\/h2>/g;
    let m;
    while ((m = re.exec(html))) out.push({ id: m[1], text: m[2].replace(/<[^>]+>/g, "") });
    return out;
  });

  // ── The guide ───────────────────────────────────────────────────
  eleventyConfig.amendLibrary("md", (md) => {
    // Raw HTML is off: the guide quotes placeholders such as « <titre> »,
    // which must print as text rather than vanish as an unknown tag.
    md.set({ html: false, typographer: false });

    // Ids on h2/h3 for the page's table of contents and deep links.
    md.core.ruler.push("heading_ids", (state) => {
      const used = new Set();
      state.tokens.forEach((tok, i) => {
        if (tok.type !== "heading_open" || !["h2", "h3"].includes(tok.tag)) return;
        let id = slugify(state.tokens[i + 1].content) || "section";
        while (used.has(id)) id += "-";
        used.add(id);
        tok.attrSet("id", id);
      });
    });

    // PNG sources also work on GitHub. Browsers can choose a smaller,
    // lossless WebP generated by `npm run images`; the PNG remains a fallback.
    const image = md.renderer.rules.image;
    md.renderer.rules.image = (tokens, idx, options, env, self) => {
      const tok = tokens[idx];
      const src = tok.attrGet("src") || "";
      const name = src.match(/(?:^|\/)images\/([\w.-]+\.png)$/);
      if (name) {
        tok.attrSet("src", `/docs/images/${name[1]}`);
        const size = pngSize(join(srcDir, "docs/images", name[1]));
        if (size) {
          tok.attrSet("width", String(size.width));
          tok.attrSet("height", String(size.height));
        }
      }
      tok.attrSet("loading", "lazy");
      tok.attrSet("decoding", "async");
      const fallback = image(tokens, idx, options, env, self);
      if (!name) return fallback;
      const size = pngSize(join(srcDir, "docs/images", name[1]));
      const variants = [480, size.width].map((width) => ({
        width,
        file: name[1].replace(/\.png$/, `-${width}.webp`),
      }));
      if (!variants.every((v) => existsSync(join(srcDir, "docs/images", v.file)))) return fallback;
      const srcset = variants.map((v) => `/docs/images/${v.file} ${v.width}w`).join(", ");
      // Guide column: 46rem maximum, sidebar + gap from 960px upward.
      const sizes = "(min-width: 1092px) 736px, (min-width: 960px) calc(100vw - 356px), (min-width: 784px) 736px, (min-width: 720px) calc(100vw - 48px), calc(100vw - 32px)";
      return `<picture><source type="image/webp" srcset="${srcset}" sizes="${sizes}">${fallback}</picture>`;
    };

    // Tables scroll sideways on their own instead of widening the page.
    md.renderer.rules.table_open = () => '<div class="table-wrap"><table>\n';
    md.renderer.rules.table_close = () => "</table></div>\n";
  });

  // Links between guide pages are written as "imports.md" so they work on
  // GitHub; on the site they become the built URL in the same language.
  eleventyConfig.addTransform("guide-links", function (content) {
    const url = this.page.url || "";
    if (!/^\/(fr\/)?docs\//.test(url) || !this.page.outputPath?.endsWith(".html")) return content;
    const base = url.startsWith("/fr/") ? "/fr/docs/" : "/docs/";
    return content.replace(/href="([\w-]+)\.md(#[\w-]+)?"/g, (m, slug, hash = "") =>
      `href="${base}${slug === "index" ? "" : `${slug}/`}${hash}"`,
    );
  });

  const byOrder = (a, b) => (a.data.order ?? 99) - (b.data.order ?? 99);
  eleventyConfig.addCollection("docs_en", (api) => api.getFilteredByTag("docs_en").sort(byOrder));
  eleventyConfig.addCollection("docs_fr", (api) => api.getFilteredByTag("docs_fr").sort(byOrder));

  return {
    dir: { input: "src", output: "_site", includes: "_includes", data: "_data" },
    templateFormats: ["njk", "md"],
    markdownTemplateEngine: false,
    htmlTemplateEngine: "njk",
  };
}
