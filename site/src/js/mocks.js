/* ─────────────────────────────────────────────────────────────
   The app screens on the landing page play the app's own steps:

     · import:   a YouTube link is typed into Import audio, the task
                 downloads, then prepares the audio, and the song lands
                 at the top of the Library;
     · trim:     the two handles close in past the intro and credits,
                 the start/end/kept values follow, the selection plays,
                 then Apply prepares the trimmed version;
     · playlist: a song is dropped into the playlist, the header says
                 Saving… while it is sent, an icon is picked for it, and
                 both settle on "Saved on Yoto".

   The markup is each scene's finished frame. Scenes only animate while
   on screen and the tab is visible, can be paused from their button,
   and stay on that finished frame under prefers-reduced-motion.
   ───────────────────────────────────────────────────────────── */
(function () {
  "use strict";

  var REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var FR = (document.documentElement.lang || "").slice(0, 2) === "fr";

  function $(root, sel) { return root.querySelector(sel); }
  function $$(root, sel) { return Array.prototype.slice.call(root.querySelectorAll(sel)); }
  function pct(n) { return Math.round(n) + (FR ? " %" : "%"); }
  function clock(s) { s = Math.round(s); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); }
  function length(s) { s = Math.round(s); return Math.floor(s / 60) + " min " + String(s % 60).padStart(2, "0"); }
  function ease(t) { return t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2; }

  /* Timers that all die together when a scene stops. */
  function Run() { this.ids = []; this.rafs = []; this.stopped = true; }
  Run.prototype.after = function (ms, fn) {
    var self = this;
    var id = setTimeout(function () { if (!self.stopped) fn(); }, ms);
    this.ids.push(id);
    return id;
  };
  Run.prototype.tween = function (ms, step, done) {
    var self = this, t0 = 0;
    var frame = function (now) {
      if (self.stopped) return;
      if (!t0) t0 = now;
      var t = Math.min(1, (now - t0) / ms);
      step(ease(t));
      if (t < 1) self.rafs.push(requestAnimationFrame(frame));
      else if (done) done();
    };
    this.rafs.push(requestAnimationFrame(frame));
  };
  Run.prototype.start = function () { this.clear(); this.stopped = false; };
  Run.prototype.clear = function () {
    this.ids.forEach(clearTimeout); this.ids = [];
    this.rafs.forEach(cancelAnimationFrame); this.rafs = [];
    this.stopped = true;
  };

  /* Where an element sits inside the scene, in px; null if it is not
     laid out at this width (a folded sidebar, a hidden button). */
  function spot(root, el, fx, fy) {
    if (!el) return null;
    var r = el.getBoundingClientRect(), o = root.getBoundingClientRect();
    if (!r.width || !r.height) return null;
    return { x: r.left - o.left + r.width * (fx == null ? 0.5 : fx), y: r.top - o.top + r.height * (fy == null ? 0.5 : fy), r: r, o: o };
  }

  /* The pointer: glides to a target, then presses it. */
  function Cursor(root) { this.root = root; this.el = $(root, "[data-cursor]"); }
  Cursor.prototype.show = function (on) { this.root.classList.toggle("has-cursor", on); };
  Cursor.prototype.place = function (x, y, instant) {
    if (!this.el) return;
    if (instant) this.el.style.transition = "none";
    this.el.style.setProperty("--cx", x - 4 + "px");
    this.el.style.setProperty("--cy", y - 3 + "px");
    if (instant) { void this.el.offsetWidth; this.el.style.transition = ""; }
  };
  Cursor.prototype.to = function (el, fx, fy) {
    var p = spot(this.root, el, fx, fy);
    if (p) this.place(p.x, p.y);
    return !!p;
  };
  Cursor.prototype.click = function (target, run) {
    var el = this.el;
    if (el) { el.classList.remove("is-click"); void el.offsetWidth; el.classList.add("is-click"); }
    if (target) {
      target.classList.add("is-pressed");
      run.after(160, function () { target.classList.remove("is-pressed"); });
    }
  };

  /* ── Base scene ─────────────────────────────────────────────── */
  function Scene(root) {
    this.root = root;
    this.run = new Run();
    this.cursor = new Cursor(root);
    this.toggle = root.parentNode.querySelector("[data-toggle]");
    this.userPaused = false;
    this.visible = false;
  }
  Scene.prototype.play = function () {
    this.run.start();
    this.root.classList.add("is-playing");
    this.cursor.show(true);
    this.reset();
    this.sequence();
  };
  Scene.prototype.stop = function () {
    this.run.clear();
    this.root.classList.remove("is-playing", "is-typing");
  };
  /* The finished frame, as authored in the markup. */
  Scene.prototype.still = function () {
    this.stop();
    this.cursor.show(false);
    this.finish();
  };
  Scene.prototype.loop = function (ms) {
    var self = this;
    this.run.after(ms, function () { self.play(); });
  };

  /* ── 1 · Import ─────────────────────────────────────────────── */
  function Import(root) {
    Scene.call(this, root);
    this.typed = $(root, "[data-typed]");
    this.text = this.typed.getAttribute("data-text");
    this.btn = $(root, "[data-import-btn]");
    this.act = $(root, "[data-activity]");
    this.line = $(root, "[data-act-line]");
    this.bar = $(root, "[data-progress]");
    this.meta = $(root, "[data-lib-meta]");
    this.nav = {};
    var self = this;
    $$(root, "[data-nav]").forEach(function (el) { self.nav[el.getAttribute("data-nav")] = el; });
    this.count = $(this.nav.library, "[data-count]");
  }
  Import.prototype = Object.create(Scene.prototype);
  Import.prototype.activeNav = function (key) {
    var nav = this.nav;
    Object.keys(nav).forEach(function (k) { nav[k].classList.toggle("is-active", k === key); });
  };
  Import.prototype.progress = function (phase, label, ms, done) {
    var self = this;
    this.act.setAttribute("data-phase", phase);
    this.run.tween(ms, function (t) {
      self.bar.style.setProperty("--p", t * 100 + "%");
      self.line.textContent = label + " · " + pct(t * 100);
    }, done);
  };
  Import.prototype.reset = function () {
    var root = this.root;
    root.setAttribute("data-view", "imports");
    root.classList.add("lib-pre");
    this.activeNav("imports");
    this.typed.textContent = "";
    this.act.classList.remove("is-shown");
    this.act.setAttribute("data-phase", "download");
    this.bar.style.setProperty("--p", "0%");
    this.meta.textContent = this.meta.getAttribute("data-before");
    if (this.count) this.count.textContent = "24";
    var o = root.getBoundingClientRect();
    this.cursor.place(o.width * 0.72, o.height * 0.85, true);
  };
  Import.prototype.sequence = function () {
    var self = this, run = this.run, root = this.root, i = 0;
    run.after(400, function () {
      root.classList.add("is-typing");
      var type = function () {
        if (i >= self.text.length) { root.classList.remove("is-typing"); return afterTyping(); }
        i += 1;
        self.typed.textContent = self.text.slice(0, i);
        run.after(28 + (i % 5 === 0 ? 40 : 0), type);
      };
      type();
    });
    var afterTyping = function () {
      self.cursor.to(self.btn);
      run.after(800, function () {
        self.cursor.click(self.btn, run);
        run.after(250, function () {
          self.act.classList.add("is-shown");
          self.progress("download", self.line.getAttribute("data-downloading"), 1500, function () {
            self.progress("prepare", self.line.getAttribute("data-preparing"), 1400, function () {
              self.act.setAttribute("data-phase", "done");
              self.line.textContent = self.line.getAttribute("data-ready");
              run.after(1100, toLibrary);
            });
          });
        });
      });
    };
    var toLibrary = function () {
      var moved = self.cursor.to(self.nav.library, 0.3);
      run.after(moved ? 800 : 200, function () {
        if (moved) self.cursor.click(null, run);
        root.setAttribute("data-view", "library");
        self.activeNav("library");
        run.after(300, function () {
          root.classList.remove("lib-pre");
          self.meta.textContent = self.meta.getAttribute("data-after");
          if (self.count) {
            self.count.textContent = "25";
            self.count.classList.remove("is-bump"); void self.count.offsetWidth; self.count.classList.add("is-bump");
          }
          self.loop(3600);
        });
      });
    };
  };
  Import.prototype.finish = function () {
    this.root.removeAttribute("data-view");
    this.root.classList.remove("lib-pre", "is-typing");
    this.activeNav("library");
    this.typed.textContent = this.text;
    this.act.removeAttribute("data-phase");
    this.line.textContent = this.line.getAttribute("data-ready");
    this.meta.textContent = this.meta.getAttribute("data-after");
    if (this.count) this.count.textContent = "25";
  };

  /* ── 2 · Trim ───────────────────────────────────────────────── */
  var DURATION = 112; // seconds: 1 min 52, the song in the design board
  function Trim(root) {
    Scene.call(this, root);
    this.wave = $(root, "[data-wave]");
    this.bars = $$(root, ".m-wave-in i");
    this.ha = $(root, ".m-handle-a");
    this.hb = $(root, ".m-handle-b");
    this.start = $(root, "[data-start]");
    this.end = $(root, "[data-end]");
    this.kept = $(root, "[data-kept]");
    this.listen = $(root, "[data-listen]");
    this.listenLabel = $(this.listen, "[data-label]");
    this.apply = $(root, "[data-apply]");
    this.note = $(root, "[data-note]");
    this.a = 0.14; this.b = 0.88; this.p = 0.38;
  }
  Trim.prototype = Object.create(Scene.prototype);
  Trim.prototype.render = function () {
    var a = this.a, b = this.b, p = this.p, n = this.bars.length;
    this.wave.style.setProperty("--a", a * 100 + "%");
    this.wave.style.setProperty("--b", b * 100 + "%");
    this.wave.style.setProperty("--p", p * 100 + "%");
    this.bars.forEach(function (bar, i) {
      var x = (i + 0.5) / n;
      var state = x < a || x > b ? "out" : x < p ? "played" : "in";
      if (!bar.classList.contains(state)) {
        bar.classList.remove("out", "in", "played");
        bar.classList.add(state);
      }
    });
    this.start.textContent = clock(a * DURATION);
    this.end.textContent = clock(b * DURATION);
    this.kept.textContent = length((b - a) * DURATION);
  };
  Trim.prototype.setNote = function (state) {
    var wrap = this.note.parentNode;
    wrap.classList.toggle("is-busy", state === "busy");
    wrap.classList.toggle("is-done", state === "done");
    this.note.textContent = this.note.getAttribute("data-" + state);
  };
  Trim.prototype.reset = function () {
    this.a = 0; this.b = 1; this.p = 0;
    this.wave.style.setProperty("--ph", "0");
    this.render();
    this.listenLabel.textContent = this.listen.getAttribute("data-listen-label");
    this.setNote("idle");
    var o = this.root.getBoundingClientRect();
    this.cursor.place(o.width * 0.5, o.height * 0.92, true);
  };
  Trim.prototype.sequence = function () {
    var self = this, run = this.run;
    run.after(300, function () {
      self.cursor.to(self.ha, 0.5, 0.5);
      run.after(750, function () {
        self.run.tween(1000, function (t) {
          self.a = 0.14 * t; self.p = self.a; self.render();
          self.cursor.to(self.ha, 0.5, 0.5);
        }, function () {
          self.cursor.to(self.hb, 0.5, 0.5);
          run.after(750, function () {
            self.run.tween(900, function (t) {
              self.b = 1 - 0.12 * t; self.render();
              self.cursor.to(self.hb, 0.5, 0.5);
            }, listen);
          });
        });
      });
    });
    var listen = function () {
      self.cursor.to(self.listen);
      run.after(800, function () {
        self.cursor.click(self.listen, run);
        self.listenLabel.textContent = self.listen.getAttribute("data-stop-label");
        self.wave.style.setProperty("--ph", "1");
        self.run.tween(2200, function (t) {
          self.p = self.a + (0.38 - self.a) * t; self.render();
        }, function () {
          self.cursor.to(self.listen);
          run.after(300, function () {
            self.cursor.click(self.listen, run);
            self.listenLabel.textContent = self.listen.getAttribute("data-listen-label");
            self.cursor.to(self.apply);
            run.after(900, apply);
          });
        });
      });
    };
    var apply = function () {
      self.cursor.click(self.apply, run);
      self.setNote("busy");
      run.after(1600, function () {
        self.setNote("done");
        self.loop(3400);
      });
    };
  };
  Trim.prototype.finish = function () {
    this.a = 0.14; this.b = 0.88; this.p = 0.38;
    this.wave.style.removeProperty("--ph");
    this.render();
    this.listenLabel.textContent = this.listen.getAttribute("data-listen-label");
    this.setNote("idle");
  };

  /* ── 3 · Playlist ───────────────────────────────────────────── */
  function Playlist(root) {
    Scene.call(this, root);
    this.status = $(root, "[data-status]");
    this.statusLabel = $(this.status, "[data-label]");
    this.meta = $(root, "[data-pl-meta]");
    this.slot = $(root, "[data-slot]");
    this.row = $(this.slot, ".m-track");
    this.icon = $(root, "[data-track-icon]");
    this.state = $(root, "[data-state]");
    this.drag = $(root, "[data-drag]");
    this.pop = $(root, "[data-pop]");
    this.icons = $$(root, "[data-icon]");
    this.use = $(root, "[data-use]");
  }
  Playlist.prototype = Object.create(Scene.prototype);
  Playlist.prototype.saving = function (on) {
    this.status.classList.toggle("is-saving", on);
    this.statusLabel.textContent = this.status.getAttribute(on ? "data-saving" : "data-saved");
  };
  Playlist.prototype.trackState = function (sending) {
    this.state.classList.toggle("f-butter", sending);
    this.state.classList.toggle("f-mint", !sending);
    this.state.textContent = this.state.getAttribute(sending ? "data-sending" : "data-done");
  };
  Playlist.prototype.pick = function (i) {
    this.icons.forEach(function (el, j) { el.classList.toggle("is-picked", i === j); });
  };
  Playlist.prototype.moveDrag = function (x, y, instant) {
    var d = this.drag;
    d.classList.toggle("is-instant", !!instant);
    d.style.setProperty("--dx", x + "px");
    d.style.setProperty("--dy", y + "px");
    if (instant) void d.offsetWidth;
  };
  Playlist.prototype.reset = function () {
    var root = this.root;
    root.classList.add("slot-closed");
    root.classList.remove("is-picking");
    this.saving(false);
    this.meta.textContent = this.meta.getAttribute("data-before");
    this.icon.classList.add("is-unset");
    this.trackState(true);
    this.pop.classList.remove("is-open");
    this.pick(3);
    var o = root.getBoundingClientRect();
    var w = this.drag.offsetWidth;
    this.startX = Math.max(8, o.width - w - 16);
    this.startY = Math.round(o.height * 0.16);
    this.drag.classList.remove("is-shown");
    this.moveDrag(this.startX, this.startY, true);
    this.cursor.place(this.startX + w * 0.55, this.startY + 26, true);
  };
  Playlist.prototype.sequence = function () {
    var self = this, run = this.run, root = this.root;
    run.after(400, function () { self.drag.classList.add("is-shown"); });
    run.after(1100, function () {
      var target = spot(root, self.slot, 0, 0);
      var tx = target ? target.x : 24, ty = target ? target.y : 200;
      root.classList.remove("slot-closed");
      self.moveDrag(tx + 4, ty + 2);
      self.cursor.place(tx + 4 + self.drag.offsetWidth * 0.55, ty + 28);
      run.after(1000, function () {
        self.drag.classList.remove("is-shown");
        self.saving(true);
        self.meta.textContent = self.meta.getAttribute("data-after");
        run.after(700, openPicker);
      });
    });
    var openPicker = function () {
      self.cursor.to(self.icon);
      run.after(750, function () {
        self.cursor.click(null, run);
        root.classList.add("is-picking");
        // Above the new row, beside its icon, the way the app's popover
        // opens: it never covers the row whose icon is being chosen.
        var p = spot(root, self.icon, 1, 0);
        var row = spot(root, self.row, 0, 0);
        var o = root.getBoundingClientRect();
        var w = self.pop.offsetWidth, h = self.pop.offsetHeight;
        var x = p ? Math.min(p.x + 12, o.width - w - 10) : 10;
        var y = row ? Math.max(36, row.y - h - 8) : 40;
        self.pop.style.setProperty("--px", Math.max(10, x) + "px");
        self.pop.style.setProperty("--py", y + "px");
        self.pop.classList.add("is-open");
        run.after(700, function () {
          self.cursor.to(self.icons[0]);
          run.after(750, function () {
            self.cursor.click(null, run);
            self.pick(0);
            run.after(500, function () {
              self.cursor.to(self.use);
              run.after(750, function () {
                self.cursor.click(self.use, run);
                run.after(200, function () {
                  self.pop.classList.remove("is-open");
                  root.classList.remove("is-picking");
                  self.icon.classList.remove("is-unset");
                  run.after(1100, function () {
                    self.trackState(false);
                    self.saving(false);
                    self.cursor.show(false);
                    self.loop(3200);
                  });
                });
              });
            });
          });
        });
      });
    };
  };
  Playlist.prototype.finish = function () {
    this.root.classList.remove("slot-closed", "is-picking");
    this.saving(false);
    this.meta.textContent = this.meta.getAttribute("data-after");
    this.icon.classList.remove("is-unset");
    this.trackState(false);
    this.pop.classList.remove("is-open");
    this.drag.classList.remove("is-shown");
    this.pick(0);
  };

  /* ── Wiring ─────────────────────────────────────────────────── */
  var kinds = { "import": Import, "trim": Trim, "playlist": Playlist };
  var scenes = $$(document, "[data-mock]").map(function (el) {
    var Kind = kinds[el.getAttribute("data-mock")];
    return Kind ? new Kind(el) : null;
  }).filter(Boolean);
  if (!scenes.length) return;

  if (REDUCED || !("IntersectionObserver" in window)) {
    scenes.forEach(function (s) { s.still(); });
    return;
  }

  var label = function (s) {
    if (!s.toggle) return;
    var paused = s.userPaused;
    s.toggle.classList.toggle("is-paused", paused);
    $(s.toggle, ".pt-label").textContent = s.toggle.getAttribute(paused ? "data-play" : "data-pause");
  };

  scenes.forEach(function (s) {
    if (!s.toggle) return;
    s.toggle.hidden = false;
    s.toggle.addEventListener("click", function () {
      s.userPaused = !s.userPaused;
      if (s.userPaused) s.stop();
      else if (s.visible && !document.hidden) s.play();
      label(s);
    });
    label(s);
  });

  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (e) {
      var s = e.target.__scene;
      if (!s) return;
      s.visible = e.isIntersecting;
      if (s.visible && !s.userPaused && !document.hidden) s.play();
      else if (!s.visible) s.stop();
    });
  }, { threshold: 0.35 });
  scenes.forEach(function (s) { s.root.__scene = s; io.observe(s.root); });

  document.addEventListener("visibilitychange", function () {
    scenes.forEach(function (s) {
      if (document.hidden) s.stop();
      else if (s.visible && !s.userPaused) s.play();
    });
  });
})();
