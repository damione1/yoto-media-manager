/* ─────────────────────────────────────────────────────────────
   The app screens on the landing page play the app's own flows,
   as its interface code (crates/yoto-desktop/src/ui) runs them:

     · import:   a YouTube link is typed into Import audio and
                 Import pressed; the button looks the link up, the
                 field clears and the task appears under Activity,
                 downloading; once ready, its Listen button opens the
                 song in the Library, with its side panel;
     · trim:     « Trim the audio » in the song's side panel opens the
                 trim dialog; the two handles are dragged past the intro
                 and the credits, a click on the waveform moves the
                 playhead, the selection plays from there, « play the end
                 of the selection » checks the cut, and Apply prepares
                 the trimmed version; once it is ready the dialog closes
                 and the song shows its trimmed version;
     · playlist: Add media opens the media dialog, the trimmed song is
                 ticked and Add (1) sends it to Yoto; the dialog closes
                 once Yoto has it and the track appears with an empty
                 icon slot; a click on the slot opens Choose an icon,
                 an icon is picked and used, and the playlist saves.

   The markup is each scene's finished frame. Scenes only animate while
   on screen and the tab is visible, can be paused from their button,
   and stay on that finished frame under prefers-reduced-motion.
   ───────────────────────────────────────────────────────────── */
(function () {
  "use strict";

  var REDUCED = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  function $(root, sel) { return root.querySelector(sel); }
  function $$(root, sel) { return Array.prototype.slice.call(root.querySelectorAll(sel)); }
  function clock(s) { s = Math.round(s); return Math.floor(s / 60) + ":" + String(s % 60).padStart(2, "0"); }
  function length(s) { s = Math.round(s); return Math.floor(s / 60) + " min " + String(s % 60).padStart(2, "0") + " s"; }
  function ease(t) { return t < 0.5 ? 2 * t * t : 1 - Math.pow(-2 * t + 2, 2) / 2; }
  function linear(t) { return t; }
  function bump(el) { if (!el) return; el.classList.remove("is-bump"); void el.offsetWidth; el.classList.add("is-bump"); }

  /* Timers that all die together when a scene stops. */
  function Run() { this.ids = []; this.rafs = []; this.stopped = true; }
  Run.prototype.after = function (ms, fn) {
    var self = this;
    var id = setTimeout(function () { if (!self.stopped) fn(); }, ms);
    this.ids.push(id);
    return id;
  };
  Run.prototype.tween = function (ms, step, done, curve) {
    var self = this, t0 = 0, f = curve || ease;
    var frame = function (now) {
      if (self.stopped) return;
      if (!t0) t0 = now;
      var t = Math.min(1, (now - t0) / ms);
      step(f(t));
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
  /* `instant` makes the pointer drag what it holds instead of easing after it. */
  Cursor.prototype.to = function (el, fx, fy, instant) {
    var p = spot(this.root, el, fx, fy);
    if (p) this.place(p.x, p.y, instant);
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

  /* A button whose label (and spinner) follow a state named in data-*. */
  function label(btn, key) {
    var l = $(btn, "[data-label]") || btn;
    l.textContent = btn.getAttribute("data-" + key);
  }

  /* ── Base scene ─────────────────────────────────────────────── */
  function Scene(root) {
    this.root = root;
    this.run = new Run();
    this.cursor = new Cursor(root);
    this.toggle = root.parentNode.querySelector("[data-toggle]");
    this.status = $(root, "[data-statusline]");
    this.ready = this.status ? this.status.textContent : "";
    this.userPaused = false;
    this.visible = false;
    var nav = this.nav = {};
    $$(root, "[data-nav]").forEach(function (el) { nav[el.getAttribute("data-nav")] = el; });
  }
  Scene.prototype.play = function () {
    this.run.start();
    this.root.classList.add("is-playing");
    this.cursor.show(true);
    // Back to the first frame at once: no transition plays the loop backwards.
    this.root.classList.add("is-resetting");
    this.reset();
    void this.root.offsetWidth;
    this.root.classList.remove("is-resetting");
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
  Scene.prototype.say = function (text) { if (this.status) this.status.textContent = text; };
  Scene.prototype.activeNav = function (key) {
    var nav = this.nav;
    Object.keys(nav).forEach(function (k) { nav[k].classList.toggle("is-active", k === key); });
  };
  Scene.prototype.count = function (key, n) {
    var el = this.nav[key] && $(this.nav[key], "[data-count]");
    if (!el) return null;
    el.textContent = String(n);
    el.classList.toggle("is-zero", n === 0);
    return el;
  };
  /* Glide to `el`, wait, press it, then run `then`. */
  Scene.prototype.press = function (el, then, wait, fx, fy) {
    var self = this, run = this.run;
    var moved = this.cursor.to(el, fx, fy);
    run.after(moved ? (wait || 800) : 150, function () {
      self.cursor.click(moved ? el : null, run);
      then();
    });
  };

  /* ── 1 · Import ─────────────────────────────────────────────── */
  function Import(root) {
    Scene.call(this, root);
    this.typed = $(root, "[data-typed]");
    this.field = $(root, "[data-field]");
    this.text = this.typed.getAttribute("data-text");
    this.btn = $(root, "[data-import-btn]");
    this.job = $(root, "[data-job]");
    this.jobTitle = $(root, "[data-job-title]");
    this.jobLine = $(root, "[data-job-line]");
    this.listen = $(root, "[data-job-listen]");
    this.meta = $(root, "[data-lib-meta]");
    this.size = $$(root, ".m-row").length;
  }
  Import.prototype = Object.create(Scene.prototype);
  Import.prototype.type = function (text) {
    this.typed.textContent = text;
    this.field.classList.toggle("is-empty", !text);
  };
  Import.prototype.jobState = function (running) {
    var j = this.job;
    j.classList.toggle("is-running", running);
    this.jobTitle.textContent = j.getAttribute(running ? "data-running-title" : "data-done-title");
    this.jobLine.textContent = j.getAttribute(running ? "data-running" : "data-done");
  };
  Import.prototype.reset = function () {
    var root = this.root;
    root.setAttribute("data-view", "imports");
    root.classList.add("lib-pre");
    this.activeNav("imports");
    this.type("");
    this.btn.classList.remove("is-busy", "is-disabled");
    label(this.btn, "idle");
    this.job.classList.remove("is-shown");
    this.jobState(true);
    this.meta.textContent = this.meta.getAttribute("data-before");
    this.count("library", this.size - 1);
    this.count("imports", 0);
    this.say(this.ready);
    var o = root.getBoundingClientRect();
    this.cursor.place(o.width * 0.72, o.height * 0.85, true);
  };
  Import.prototype.sequence = function () {
    var self = this, run = this.run, root = this.root, i = 0;
    run.after(400, function () {
      root.classList.add("is-typing");
      var type = function () {
        if (i >= self.text.length) { root.classList.remove("is-typing"); return lookUp(); }
        i += 1;
        self.type(self.text.slice(0, i));
        run.after(28 + (i % 5 === 0 ? 40 : 0), type);
      };
      type();
    });
    // « Importer » first looks the link up, then starts the import and
    // clears the field; the task shows under Activity.
    var lookUp = function () {
      self.press(self.btn, function () {
        self.btn.classList.add("is-busy", "is-disabled");
        label(self.btn, "busy");
        self.say(self.btn.getAttribute("data-say-busy"));
        run.after(1000, started);
      });
    };
    var started = function () {
      self.btn.classList.remove("is-busy", "is-disabled");
      label(self.btn, "idle");
      self.type("");
      self.say(self.btn.getAttribute("data-say-started"));
      self.jobState(true);
      self.job.classList.add("is-shown");
      bump(self.count("imports", 1));
      run.after(2600, done);
    };
    var done = function () {
      self.jobState(false);
      self.count("imports", 0);
      self.meta.textContent = self.meta.getAttribute("data-after");
      bump(self.count("library", self.size));
      run.after(900, function () {
        // « Écouter » opens the song in the Library, in its side panel.
        self.press(self.listen, function () {
          self.say(self.ready);
          root.setAttribute("data-view", "library");
          self.activeNav("library");
          run.after(300, function () {
            root.classList.remove("lib-pre");
            self.cursor.show(false);
            self.loop(4200);
          });
        });
      });
    };
  };
  Import.prototype.finish = function () {
    this.root.removeAttribute("data-view");
    this.root.classList.remove("lib-pre", "is-typing");
    this.activeNav("library");
    // The app clears the link once its import has started.
    this.type("");
    this.btn.classList.remove("is-busy", "is-disabled");
    label(this.btn, "idle");
    this.job.classList.add("is-shown");
    this.jobState(false);
    this.meta.textContent = this.meta.getAttribute("data-after");
    this.count("library", this.size);
    this.count("imports", 0);
    this.say(this.ready);
  };

  /* ── 2 · Trim ───────────────────────────────────────────────── */
  var DURATION = 112;                 // 1 min 52 s
  var CUT_A = 16 / DURATION;          // 0:16
  var CUT_B = 99 / DURATION;          // 1:39
  var SEEK = 0.45;                    // where the waveform is clicked
  var ENDING = 5 / DURATION;          // the app plays the last 5 seconds
  function Trim(root) {
    Scene.call(this, root);
    this.box = $(root, "[data-trim]");
    this.wave = $(root, "[data-wave]");
    this.bars = $$(root, ".m-wave-in i");
    this.ha = $(root, ".m-handle-a");
    this.hb = $(root, ".m-handle-b");
    this.start = $(root, "[data-start]");
    this.end = $(root, "[data-end]");
    this.kept = $(root, "[data-kept]");
    this.listenEnd = $(root, "[data-listen-end]");
    this.listen = $(root, "[data-listen]");
    this.foot = $(root, "[data-foot]");
    this.openTrim = $(root, "[data-open-trim]");
    this.note = $(root, "[data-note]");
    this.cancel = $(root, "[data-cancel]");
    this.apply = $(root, "[data-apply]");
  }
  Trim.prototype = Object.create(Scene.prototype);
  Trim.prototype.render = function () {
    var a = this.a, b = this.b, p = this.p, n = this.bars.length;
    // The playhead is drawn only while it sits inside the selection.
    var head = p >= a - 1e-6 && p <= b + 1e-6;
    this.box.style.setProperty("--a", a * 100 + "%");
    this.box.style.setProperty("--b", b * 100 + "%");
    this.wave.style.setProperty("--a", a * 100 + "%");
    this.wave.style.setProperty("--b", b * 100 + "%");
    this.wave.style.setProperty("--p", p * 100 + "%");
    this.wave.classList.toggle("no-head", !head);
    this.bars.forEach(function (bar, i) {
      var x = (i + 0.5) / n;
      var state = x < a || x > b ? "out" : head && x <= p ? "played" : "in";
      if (!bar.classList.contains(state)) {
        bar.classList.remove("out", "in", "played");
        bar.classList.add(state);
      }
    });
    this.start.textContent = clock(a * DURATION);
    this.end.textContent = clock(b * DURATION);
    this.kept.textContent = length((b - a) * DURATION);
  };
  Trim.prototype.playing = function (on) {
    this.listen.classList.toggle("is-on", on);
    label(this.listen, on ? "playing" : "idle");
  };
  Trim.prototype.prep = function (busy) {
    this.foot.classList.toggle("is-busy", busy);
    this.note.textContent = this.note.getAttribute(busy ? "data-busy" : "data-idle");
    this.apply.classList.toggle("is-busy", busy);
    this.apply.classList.toggle("is-disabled", busy);
    label(this.apply, busy ? "busy" : "idle");
    [this.cancel, this.listen, this.listenEnd].forEach(function (b) { b.classList.toggle("is-disabled", busy); });
  };
  Trim.prototype.dialog = function (open) {
    this.box.classList.toggle("is-open", open);
    this.root.classList.toggle("has-dialog", open);
  };
  Trim.prototype.reset = function () {
    this.root.classList.remove("is-cut");
    this.dialog(false);
    this.a = 0; this.b = 1; this.p = 0;
    this.render();
    this.playing(false);
    this.prep(false);
    var o = this.root.getBoundingClientRect();
    this.cursor.place(o.width * 0.5, o.height * 0.92, true);
  };
  Trim.prototype.sequence = function () {
    var self = this, run = this.run;
    // Each handle is grabbed on the waveform and dragged.
    var drag = function (handle, from, to, set, ms, then) {
      self.cursor.to(handle, 0.5, 0.5);
      run.after(750, function () {
        run.tween(ms, function (t) {
          set(from + (to - from) * t); self.render();
          self.cursor.to(handle, 0.5, 0.5, true);
        }, then);
      });
    };
    run.after(400, function () {
      self.press(self.openTrim, function () {
        self.dialog(true);
        run.after(600, function () {
          drag(self.ha, 0, CUT_A, function (v) { self.a = v; }, 1000, function () {
            drag(self.hb, 1, CUT_B, function (v) { self.b = v; }, 900, seek);
          });
        });
      });
    });
    // A click anywhere else on the waveform moves the playhead.
    var seek = function () {
      var p = spot(self.root, self.wave, 0, 0.5);
      var w = $(self.wave, ".m-wave-in").getBoundingClientRect();
      if (p) self.cursor.place(w.left - p.o.left + w.width * SEEK, p.y + 12);
      run.after(800, function () {
        self.cursor.click(null, run);
        self.p = SEEK; self.render();
        run.after(500, listen);
      });
    };
    // Listening starts from the playhead.
    var listen = function () {
      self.press(self.listen, function () {
        self.playing(true);
        run.tween(2000, function (t) {
          self.p = SEEK + 0.12 * t; self.render();
        }, ending, linear);
      });
    };
    // « Écouter la fin de la sélection »: the last seconds, to check the cut.
    var ending = function () {
      self.press(self.listenEnd, function () {
        self.p = CUT_B - ENDING; self.render();
        run.tween(2300, function (t) {
          self.p = CUT_B - ENDING + ENDING * t; self.render();
        }, function () {
          self.playing(false);
          run.after(500, apply);
        }, linear);
      }, 700);
    };
    // Once the trimmed version is ready, the app closes the dialog: the
    // song now shows « Découpé », its trimmed version and a note.
    var apply = function () {
      self.press(self.apply, function () {
        self.prep(true);
        run.after(1800, function () {
          self.dialog(false);
          self.root.classList.add("is-cut");
          self.cursor.show(false);
          self.loop(3800);
        });
      });
    };
  };
  /* The finished frame is the dialog with the cut chosen, before Apply. */
  Trim.prototype.finish = function () {
    this.root.classList.remove("is-cut");
    this.dialog(true);
    this.a = CUT_A; this.b = CUT_B; this.p = SEEK;
    this.render();
    this.playing(false);
    this.prep(false);
  };

  /* ── 3 · Playlist ───────────────────────────────────────────── */
  function Playlist(root) {
    Scene.call(this, root);
    this.status = $(root, "[data-statusline]");
    this.tag = $(root, "[data-status]");
    this.tagLabel = $(this.tag, "[data-label]");
    this.meta = $(root, "[data-pl-meta]");
    this.add = $(root, "[data-add]");
    this.picker = $(root, "[data-picker]");
    this.target = $(root, "[data-pick-row]");
    this.targetCheck = $(this.target, ".m-check");
    this.addN = $(root, "[data-pick-add]");
    this.slot = $(root, "[data-track-icon]");
    this.icons = $(root, "[data-icons]");
    this.cells = $$(root, "[data-icon]");
    this.use = $(root, "[data-use]");
    this.pickName = root.getAttribute("data-pick");
  }
  Playlist.prototype = Object.create(Scene.prototype);
  Playlist.prototype.saving = function (on) {
    this.tag.classList.toggle("is-saving", on);
    this.tagLabel.textContent = this.tag.getAttribute(on ? "data-saving" : "data-saved");
  };
  Playlist.prototype.dialog = function (el, open) {
    el.classList.toggle("is-open", open);
    this.root.classList.toggle("has-dialog", open);
  };
  Playlist.prototype.tick = function (on) {
    this.target.classList.toggle("is-checked", on);
    this.addN.classList.toggle("is-disabled", !on);
    label(this.addN, on ? "one" : "zero");
  };
  Playlist.prototype.pick = function (name) {
    this.cells.forEach(function (el) { el.classList.toggle("is-picked", el.getAttribute("data-icon") === name); });
    this.use.classList.toggle("is-disabled", !name);
  };
  Playlist.prototype.cell = function (name) {
    return this.cells.filter(function (el) { return el.getAttribute("data-icon") === name; })[0];
  };
  Playlist.prototype.reset = function () {
    var root = this.root;
    root.classList.add("is-short");
    root.classList.remove("is-choosing", "has-dialog");
    this.picker.classList.remove("is-open", "is-busy");
    this.icons.classList.remove("is-open");
    this.saving(false);
    this.meta.textContent = this.meta.getAttribute("data-before");
    this.tick(false);
    this.slot.classList.remove("is-set");
    this.pick(null);
    var o = root.getBoundingClientRect();
    this.cursor.place(o.width * 0.55, o.height * 0.8, true);
  };
  Playlist.prototype.sequence = function () {
    var self = this, run = this.run, root = this.root;
    // « Ajouter des médias » → the dialog → tick the song → « Ajouter (1) ».
    run.after(500, function () {
      self.press(self.add, function () {
        self.dialog(self.picker, true);
        run.after(700, function () {
          self.press(self.targetCheck, function () {
            self.tick(true);
            run.after(400, send);
          });
        });
      });
    });
    // The audio is sent to Yoto before it appears in the playlist: the
    // dialog waits, then closes, and the new track is there, on Yoto.
    var send = function () {
      self.press(self.addN, function () {
        self.picker.classList.add("is-busy");
        self.addN.classList.add("is-disabled");
        label(self.addN, "busy");
        self.saving(true);
        run.after(2300, function () {
          self.dialog(self.picker, false);
          self.picker.classList.remove("is-busy");
          root.classList.remove("is-short");
          self.saving(false);
          self.meta.textContent = self.meta.getAttribute("data-after");
          run.after(1100, chooseIcon);
        });
      });
    };
    // Its empty icon slot opens « Choisir une icône ».
    var chooseIcon = function () {
      self.press(self.slot, function () {
        root.classList.add("is-choosing");
        self.dialog(self.icons, true);
        run.after(700, function () {
          self.press(self.cell(self.pickName), function () {
            self.pick(self.pickName);
            run.after(400, function () {
              self.press(self.use, function () {
                self.dialog(self.icons, false);
                root.classList.remove("is-choosing");
                self.slot.classList.add("is-set");
                // The change is grouped and saved on its own.
                run.after(400, function () {
                  self.saving(true);
                  run.after(1300, function () {
                    self.saving(false);
                    self.cursor.show(false);
                    self.loop(3200);
                  });
                });
              }, 750, 0.7, 0.6);
            });
          });
        });
      });
    };
  };
  Playlist.prototype.finish = function () {
    this.root.classList.remove("is-short", "is-choosing", "has-dialog");
    this.picker.classList.remove("is-open", "is-busy");
    this.icons.classList.remove("is-open");
    this.saving(false);
    this.meta.textContent = this.meta.getAttribute("data-after");
    this.tick(false);
    this.slot.classList.add("is-set");
    this.pick(null);
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

  var toggleLabel = function (s) {
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
      toggleLabel(s);
    });
    toggleLabel(s);
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
