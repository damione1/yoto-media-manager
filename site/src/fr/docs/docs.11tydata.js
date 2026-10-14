import locales from "../../_data/locales.js";

/** The French guide: same file names as src/docs/, served under /fr/docs/. */
export default {
  layout: "layouts/doc.njk",
  isDocs: true,
  loc: locales.find((l) => l.code === "fr"),
  tags: ["docs_fr"],
  permalink: (data) =>
    data.page.filePathStem.endsWith("/index") ? "/fr/docs/" : `/fr/docs/${data.page.fileSlug}/`,
};
