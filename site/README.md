# Site YotoMediaManager

The public site is built with Eleventy. English lives under `src/docs/`, French
under `src/fr/docs/`; each page has its own `title` and `description`.
`navTitle` keeps the guide sidebar concise. Existing URLs stay stable.

```sh
npm ci --ignore-scripts
npm test
npm run preview
```

`npm run preview` serves the built site through the local Cloudflare Worker at
http://localhost:8787, including permanent redirects, security headers and real
404 responses. `npm run dev` remains available for Eleventy live reload alone.

## Hosting

`wrangler.jsonc` targets the existing **yoto-media-manager** Worker. Cloudflare
Workers Builds uses `site/` as its root, `npm run build` as the build command
and `npx wrangler deploy` as its deployment command. Keep the production
`SITE_URL=https://yotomediamanager.damiengoehrig.ca` build variable for canonical
URLs, language alternates, Open Graph images and the sitemap. Preview builds
use the same public canonicals unless this variable is explicitly overridden.

The build emits `_redirects` for known page aliases only. Asset routing uses
`404-page` to return the nearest localized `404.html` with status 404. The HTML
Worker adds a fresh CSP nonce to each response; Cloudflare JavaScript Detections
reads this nonce and applies it to the scripts it injects. HTML responses use
`no-store` so the nonce is not reused through a browser cache. Static resources
skip the Worker and retain their asset cache policies. `security-headers.js`
is the common source for Worker headers and the generated `_headers` file.

For a manually authorized deployment, run `npm run deploy` from this directory.
Adding this configuration does not itself publish a deployment.

## Guide screenshots

Keep the PNG originals: Markdown links remain readable on GitHub and browsers
have a fallback. After adding or replacing a screenshot, run `npm run images`
with Pillow available in your Python environment, then commit the generated
480px and full-width lossless WebP variants. Production builds only copy these
checked-in files; they do not need Python or an image service. The renderer uses
`picture`, `srcset`, `sizes`, lazy loading and explicit dimensions.

References: [Workers assets and custom 404s](https://developers.cloudflare.com/workers/static-assets/routing/static-site-generation/),
[permanent redirects](https://developers.cloudflare.com/workers/static-assets/redirects/),
[Cloudflare JavaScript Detections and CSP](https://developers.cloudflare.com/cloudflare-challenges/challenge-types/javascript-detections/).
