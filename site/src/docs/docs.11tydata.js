import locales from "../_data/locales.js";

/** Every Markdown file in this folder is an English guide page:
 *  index.md → /docs/, imports.md → /docs/imports/. */
export default {
  layout: "layouts/doc.njk",
  isDocs: true,
  loc: locales.find((l) => l.code === "en"),
  tags: ["docs_en"],
  permalink: (data) =>
    data.page.filePathStem.endsWith("/index") ? "/docs/" : `/docs/${data.page.fileSlug}/`,
};
