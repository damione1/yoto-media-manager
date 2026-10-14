/* The guide: the page list is a disclosure on phones and an always-open
   sidebar on wide screens. CSS opens it where ::details-content is
   supported; this covers the browsers where it is not. */
(function () {
  "use strict";
  var menu = document.querySelector(".docs-menu");
  if (!menu) return;
  var wide = window.matchMedia("(min-width: 960px)");
  var sync = function () { if (wide.matches) menu.open = true; };
  sync();
  if (wide.addEventListener) wide.addEventListener("change", sync);
})();

/* The guide: underline the section you are reading in "On this page". */
(function () {
  "use strict";
  var links = Array.prototype.slice.call(document.querySelectorAll(".docs-toc a"));
  if (!links.length || !("IntersectionObserver" in window)) return;
  var byId = {};
  links.forEach(function (a) { byId[decodeURIComponent(a.getAttribute("href").slice(1))] = a; });
  var heads = Array.prototype.slice.call(document.querySelectorAll(".prose h2[id]"));
  var seen = {};
  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) { seen[e.target.id] = e.isIntersecting; });
    var current = null;
    heads.forEach(function (h) { if (!current && seen[h.id]) current = h.id; });
    if (!current) return;
    links.forEach(function (a) { a.classList.remove("is-active"); a.removeAttribute("aria-current"); });
    if (byId[current]) { byId[current].classList.add("is-active"); byId[current].setAttribute("aria-current", "location"); }
  }, { rootMargin: "0px 0px -65% 0px" });
  heads.forEach(function (h) { io.observe(h); });
})();
