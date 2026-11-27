/*!
 * textplayer — a text-mode (DOS-style) audio player window for plain links to audio files.
 * No dependencies, no images: the window is drawn with box-drawing characters.
 * MIT License.
 *
 * Usage: load this file, then call
 *   TextPlayer.attach({ links: "a[href$='.mp3']" });
 * See README.md for data attributes, options and CSS variables.
 *
 * Without JavaScript the links keep working as plain links to the files.
 * This file may be inlined into a page: it must never contain a closing script tag.
 */
(function (global) {
  "use strict";

  const DEFAULTS = {
    links: "a[data-tp]",                 // CSS selector or an array of <a> elements
    title: "Browser Player v0.1",        // shown in the top frame
    item: "li",                          // closest element that gets `playingClass`
    playingClass: "tp-playing",
    width: 70,                           // inner width of the window, in columns
    bufferTarget: 60,                    // seconds buffered ahead that count as a full "Buf"
    spectrum: true,                      // spectrum analyzer under "Buf" (same-origin audio only)
    spectrumRows: 16,                    // segments per bar (a segment is about a third of a line)
    spectrumCookie: "tp_spectrum",       // cookie that keeps the A key choice (shown / hidden)
  };

  // Look of the window. Every color and the font can be changed with CSS variables
  // (--tp-*) on :root or on .tp-overlay, without touching this file.
  const CSS = `
.tp-overlay { position: fixed; inset: 0; z-index: var(--tp-z, 1000); display: flex;
  justify-content: center; align-items: center; padding: 1rem; background: var(--tp-backdrop, rgb(0 0 0 / .55)); }
/* author display:flex would otherwise override the hidden attribute */
.tp-overlay[hidden] { display: none; }
.tp-win { margin: 0; padding: .5lh 1ch; background: var(--tp-bg, #aaa); color: var(--tp-label, #00a);
  user-select: none; font: bold var(--tp-font-size, min(16px, calc((100vw - 2rem) / 46))) / normal
  var(--tp-font, "DejaVu Sans Mono", Menlo, Consolas, "Liberation Mono", monospace); }
.tp-win:focus { outline: none; }
.tp-win .tp-fr { color: var(--tp-frame, #fff); font-weight: normal; }
.tp-win .tp-ttl { background: var(--tp-title-bg, #fff); color: var(--tp-title, #555); }
.tp-win .tp-va { color: var(--tp-value, #ff5); }
.tp-win .tp-help { color: var(--tp-help, #000); }
.tp-win .tp-slider { cursor: pointer; touch-action: none; }
/* the state glyph may come from a fallback font with a taller line (Menlo lacks
   some of these): keep it out of the line height, or the window jumps on pause */
.tp-win .tp-st { line-height: 0; }
/* spectrum analyzer: segments of LED bars. The whole analyzer is one inline block
   with its own, tight line height: a segment is "▂" (a quarter of a cell), so the
   segments come thick with thin gaps; the frame's "║" overlap and stay one line. */
.tp-win .tp-specbox { display: inline-block; vertical-align: top; line-height: .42em;
  padding: .3em 0 .35em; }
.tp-win .tp-g { color: var(--tp-spec-low, #1a1); }
.tp-win .tp-y { color: var(--tp-spec-mid, #cc0); }
.tp-win .tp-r { color: var(--tp-spec-high, #d22); }
`;

  // Browser name for the "Decoder:" line.
  function browserName() {
    const ua = global.navigator ? global.navigator.userAgent : "";
    const known = [[/Edg\//, "Edge"], [/OPR\/|Opera/, "Opera"], [/YaBrowser/, "Yandex"],
                   [/Firefox\//, "Firefox"], [/Chrome\/|Chromium\//, "Chrome"], [/Safari\//, "Safari"]];
    for (const [re, name] of known) if (re.test(ua)) return name;
    return "Browser";
  }

  // --- text helpers -----------------------------------------------------------
  const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
  const fit = (s, n) => { s = String(s ?? ""); return s.length > n ? s.slice(0, n - 1) + "…" : s; };
  // A segment of a row: text, CSS class, extra attributes.
  const seg = (t, cls = "", attrs = "") => ({ t: String(t), cls, attrs });
  const html = (s) => (s.cls || s.attrs ? `<span class="${s.cls}" ${s.attrs}>${esc(s.t)}</span>` : esc(s.t));
  const width = (segs) => segs.reduce((n, s) => n + [...s.t].length, 0);
  const bar = (frac, n) => {
    const k = Math.round(Math.min(1, Math.max(0, frac || 0)) * n);
    return "█".repeat(k) + "░".repeat(n - k);
  };
  const time = (s) => {
    if (!isFinite(s)) return "--:--";
    s = Math.floor(s);
    const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60, x = String(s % 60).padStart(2, "0");
    return h ? `${h}:${String(m).padStart(2, "0")}:${x}` : `${String(m).padStart(2, "0")}:${x}`;
  };
  const fileName = (href) => {
    try { return decodeURIComponent(new URL(href, global.location.href).pathname.split("/").pop()); }
    catch { return href; }
  };

  function readCookie(name) {
    try {
      const m = global.document.cookie.match(new RegExp("(?:^|; )" + name.replace(/\W/g, "") + "=([^;]*)"));
      return m ? m[1] : null;
    } catch { return null; }
  }
  function writeCookie(name, value) {
    try {
      global.document.cookie = `${name.replace(/\W/g, "")}=${value}; path=/; max-age=31536000; SameSite=Lax`;
    } catch { /* cookies blocked: the choice lasts until the page is left */ }
  }

  function injectStyle(doc) {
    if (doc.getElementById("tp-style")) return;
    const style = doc.createElement("style");
    style.id = "tp-style";
    style.textContent = CSS;
    doc.head.append(style);
  }

  /**
   * Turns the links into a playlist played in the text-mode window.
   * Episode info comes from the links' data attributes (all optional):
   *   data-file, data-title, data-artist, data-podcast, data-date,
   *   data-size (bytes), data-format (e.g. "MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Stereo").
   * Returns a controller { open(index), close(), audio } or null if there are no links.
   */
  function attach(options) {
    const o = Object.assign({}, DEFAULTS, options);
    const doc = global.document;
    const links = typeof o.links === "string" ? [...doc.querySelectorAll(o.links)] : [...o.links];
    if (!links.length) return null;
    injectStyle(doc);

    const W = o.width;
    const BROWSER = browserName();
    const audio = new Audio();
    audio.preload = "auto";
    let cur = -1;

    const overlay = doc.createElement("div");
    overlay.className = "tp-overlay";
    overlay.hidden = true;
    overlay.innerHTML = '<pre class="tp-win" role="dialog" aria-modal="true" aria-label="Audio player"></pre>';
    doc.body.append(overlay);
    const win = overlay.querySelector(".tp-win");
    win.tabIndex = -1;                   // focusable, so focus can leave the clicked link

    const row = (...segs) => {
      const pad = Math.max(0, W - 2 - width(segs));
      return `<span class="tp-fr">║</span> ${segs.map(html).join("")}${" ".repeat(pad)} <span class="tp-fr">║</span>`;
    };
    // Groups of segments across the inner width: the spare columns go evenly between them.
    const spread = (...groups) => {
      const free = Math.max(0, W - 2 - groups.reduce((n, g) => n + width(g), 0));
      const gaps = groups.length - 1;
      return groups.flatMap((g, i) => i < gaps
        ? [...g, seg(" ".repeat(Math.floor(free / gaps) + (i < free % gaps ? 1 : 0)))]
        : g);
    };
    const sep = () => `<span class="tp-fr">╟${"─".repeat(W)}╢</span>`;
    const L = (s) => seg(s, "tp-lb"), V = (s) => seg(s, "tp-va");
    // Key help is plain text: keyboard only, the mouse does nothing here.
    // "tp-st": arrow glyphs may come from a fallback font; keep them out of the line height
    const keys = (...pairs) => pairs.map(([k, label]) => seg(`${k}:${label}`, "tp-help tp-st"))
      .flatMap((s, i) => (i ? [seg(" | ", "tp-fr"), s] : [s]));   // separators in the frame color
    const itemOf = (a) => (o.item && a.closest(o.item)) || a;

    function render() {
      const link = links[cur], a = link.dataset;
      const dur = audio.duration, size = Number(a.size);
      const kbps = size > 0 && isFinite(dur) && dur > 0 ? Math.round((size * 8) / dur / 1000) : null;
      const db = audio.volume > 0 ? Math.round(20 * Math.log10(audio.volume)) + "dB" : "mute";
      // Buffer health: audio buffered ahead of the playhead. Browsers do not download
      // the whole file, they keep a window ahead (Chrome: about 1.5 minutes), so the
      // share of the file loaded so far would only follow the playhead.
      const now = audio.currentTime;
      let ahead = 0;
      for (let i = 0; i < audio.buffered.length; i++) {
        if (audio.buffered.start(i) <= now + 0.5 && now <= audio.buffered.end(i)) {
          ahead = audio.buffered.end(i) - now;
        }
      }
      // Not downloading while it has enough to play: the browser holds all it wants
      // (it suspends at its own limit, and near the end there is nothing left to get).
      const idle = audio.networkState === audio.NETWORK_IDLE && audio.readyState >= audio.HAVE_FUTURE_DATA;
      const buf = idle ? 1 : Math.min(1, ahead / o.bufferTarget);
      // Near the end browsers report little buffered ahead even when all of it is
      // there: with nothing left to download, what is ahead is the rest of the track.
      if (idle && isFinite(dur)) ahead = Math.max(ahead, Math.min(dur - now, o.bufferTarget));

      const title = ` ${fit(o.title, W - 4)} `;
      const left = Math.floor((W - title.length) / 2);
      const ct = time(audio.currentTime), dt = time(dur);
      const pw = W - 2 - 2 - ct.length - 1 - 1 - dt.length;   // "► " time " " bar " " time

      win.innerHTML = [
        `<span class="tp-fr">╔${"═".repeat(left)}</span><span class="tp-ttl">${esc(title)}</span>` +
          `<span class="tp-fr">${"═".repeat(W - left - title.length)}╗</span>`,
        row(L("File:    "), V(fit(a.file || fileName(link.href), W - 11))),
        sep(),
        row(L("Title:   "), V(fit(a.title || link.textContent.trim(), W - 11))),
        row(L("Artist:  "), V(fit(a.artist, W - 11))),
        row(L("Podcast: "), V(fit(a.podcast, W - 11))),
        row(L("Date:    "), V((a.date || "").padEnd(14)), L("Track: "), V(`${cur + 1}/${links.length}`.padEnd(10)),
            L("Size: "), V(size > 0 ? `${Math.round(size / 1e6)} MB` : "")),
        sep(),
        // stream info from the page (e.g. parsed from the MPEG header); else an estimate
        row(V(fit(a.format || (kbps ? `${kbps} kbps` : ""), W - 2))),
        // decoder left, buffer (time buffered ahead, as on the progress line) in the
        // middle, volume right: the free columns are shared out between them
        row(...spread(
          [L("Decoder: "), V(BROWSER)],
          [L("Buf: "), seg(bar(buf, 13), "tp-lb"), V(" " + time(ahead))],
          [L("Vol: "), seg(bar(audio.volume, 13), "tp-lb tp-slider", 'data-act="vol"'), V(" " + db.padStart(5))],
        )),
        row(),
        row(seg(audio.paused ? "║" : "►", "tp-lb tp-st"), L(" " + ct + " "),
            seg(bar(audio.currentTime / dur, pw), "tp-lb tp-slider", 'data-act="seek"'), L(" " + dt)),
        ...(spec.on && spec.shown ? [row(), `<span class="tp-specbox">${Array.from({ length: spec.rows }, (_, r) =>
          `<span class="tp-fr">║</span> <span class="tp-spec">${spec.line(r)}</span> <span class="tp-fr">║</span>`).join("\n")}</span>`] : []),
        sep(),
        row(...keys(["Spc", "next"], ["P", "prev"], ["←→↑↓", "seek"], ["S", "pause"], ["+-", "vol"],
                    ...(spec.on ? [["A", "spec"]] : []), ["Esc", "exit"])),
        `<span class="tp-fr">╚${"═".repeat(W)}╝</span>`,
      ].join("\n");
    }

    // --- spectrum analyzer ----------------------------------------------------
    // Web Audio: the element feeds an AnalyserNode. Routing an element through Web
    // Audio is for good, and audio from another origin (without CORS) would then come
    // out silent, so it is used only when every link is on this site.
    const sameOrigin = links.every((a) => {
      try { return new URL(a.href, global.location.href).origin === global.location.origin; }
      catch { return false; }
    });
    const spec = {
      on: o.spectrum && sameOrigin && !!(global.AudioContext || global.webkitAudioContext),
      shown: readCookie(o.spectrumCookie) !== "0",   // the A key; shown unless turned off
      rows: o.spectrumRows,
      // a bar is 2 columns, bars 1 apart: as many as fit the inner width (23 at 70),
      // so the spectrum lines up with the text above it, edge to edge
      bands: Math.floor((W - 1) / 3),
      levels: null,                         // per band, 0..rows
      peaks: null, held: null,              // peak level per band, when it was reached
      ctx: null, an: null, data: null, raf: 0, last: 0,
      // one text line of the bars, row r from the top: colored "▂▂" segments
      line(r) {
        const t = this.rows - r;            // the level this line lights at
        const cls = t > this.rows * 0.8 ? "tp-r" : t > this.rows * 0.5 ? "tp-y" : "tp-g";
        const cells = [];
        for (let b = 0; b < this.bands; b++) {
          const level = this.levels ? Math.round(this.levels[b]) : 0;
          const peak = this.peaks ? Math.round(this.peaks[b]) : 0;
          // a bar up to its level, and above it the hanging peak segment
          cells.push(level >= t || (peak === t && peak > level) ? "▂▂" : "  ");
        }
        // bars centered in the inner width: the spare columns split left and right
        const bars = cells.join(" "), left = Math.floor((W - 2 - bars.length) / 2);
        return `<span class="${cls}">${(" ".repeat(left) + bars).padEnd(W - 2)}</span>`;
      },
    };
    // A: show / hide the analyzer, remembered in a cookie for a year. The window is
    // drawn anew, so it grows or shrinks to fit.
    function specToggle() {
      if (!spec.on) return;
      spec.shown = !spec.shown;
      writeCookie(o.spectrumCookie, spec.shown ? "1" : "0");
      render();
      if (spec.shown) specStart();
    }
    function specStart() {
      if (!spec.on || !spec.shown) return;
      if (!spec.an) {
        try {
          const AC = global.AudioContext || global.webkitAudioContext;
          spec.ctx = new AC();
          const src = spec.ctx.createMediaElementSource(audio);
          spec.an = spec.ctx.createAnalyser();
          spec.an.fftSize = 4096;
          spec.an.smoothingTimeConstant = 0.5;
          src.connect(spec.an);
          spec.an.connect(spec.ctx.destination);
          spec.data = new Float32Array(spec.an.frequencyBinCount);
          spec.levels = new Array(spec.bands).fill(0);
          spec.peaks = new Array(spec.bands).fill(0);
          spec.held = new Array(spec.bands).fill(0);
        } catch {
          spec.on = false;                  // the player still plays, without bars
          return;
        }
      }
      if (spec.ctx.state === "suspended") spec.ctx.resume().catch(() => {});
      if (!spec.raf) spec.raf = global.requestAnimationFrame(specTick);
    }
    // Analyzer tuning. Levels are average band power in dB with a tilt of +4.5 dB per
    // octave around 1 kHz (music loses energy towards the highs; without the tilt the
    // right half stays low), mapped from FLOOR..CEIL onto the segments. Measured on the
    // podcasts: after the tilt the median is about -55 dB, 95% below -42, peaks -31.
    const SPEC = {
      FLOOR: -80, CEIL: -36,      // dB after the tilt: empty .. full bar
      TILT: 4.5,                  // dB per octave
      RELEASE: 1.6,               // bars fall this many full heights per second (rise at once)
      HOLD: 0.8,                  // seconds a peak stays put
      PEAK_FALL: 0.5,             // then it falls this many full heights per second
    };
    function specTick(now) {
      spec.raf = 0;
      if (overlay.hidden || !spec.an || !spec.shown) { spec.last = 0; return; }
      // time-based ballistics: the same on 60 and 120 Hz screens
      const dt = spec.last ? Math.min(0.1, (now - spec.last) / 1000) : 0;
      spec.last = now;
      spec.an.getFloatFrequencyData(spec.data);
      // bands spaced evenly on a log scale, 50 Hz .. 16 kHz
      const nyquist = spec.ctx.sampleRate / 2, n = spec.data.length, rows = spec.rows;
      for (let b = 0; b < spec.bands; b++) {
        const f0 = 50 * Math.pow(320, b / spec.bands), f1 = 50 * Math.pow(320, (b + 1) / spec.bands);
        const i0 = Math.floor((f0 / nyquist) * n), i1 = Math.max(i0 + 1, Math.ceil((f1 / nyquist) * n));
        let power = 0, count = 0;
        for (let i = i0; i < i1 && i < n; i++) { power += Math.pow(10, spec.data[i] / 10); count++; }
        let db = count && power > 0 ? 10 * Math.log10(power / count) : -Infinity;
        db += SPEC.TILT * Math.log2(Math.sqrt(f0 * f1) / 1000);
        const target = Math.min(1, Math.max(0, (db - SPEC.FLOOR) / (SPEC.CEIL - SPEC.FLOOR))) * rows;
        spec.levels[b] = Math.max(target, spec.levels[b] - rows * SPEC.RELEASE * dt);
        // peak hold: jumps up with the bar, stays HOLD seconds, then falls slowly
        if (spec.levels[b] >= spec.peaks[b]) { spec.peaks[b] = spec.levels[b]; spec.held[b] = now; }
        else if (now - spec.held[b] > SPEC.HOLD * 1000) {
          spec.peaks[b] = Math.max(spec.levels[b], spec.peaks[b] - rows * SPEC.PEAK_FALL * dt);
        }
      }
      const lines = win.querySelectorAll(".tp-spec");
      lines.forEach((el, r) => { el.innerHTML = spec.line(r); });
      spec.raf = global.requestAnimationFrame(specTick);
    }


    function open(i) {
      if (i < 0 || i >= links.length) return;
      if (cur >= 0) itemOf(links[cur]).classList.remove(o.playingClass);
      cur = i;
      itemOf(links[cur]).classList.add(o.playingClass);
      audio.src = links[cur].href;
      audio.play().catch(() => {});
      overlay.hidden = false;
      specStart();                          // a click or a key: Web Audio may start now
      // Otherwise the clicked link keeps focus and, after the first key press, its
      // :focus-visible highlight looks like a second playing track.
      win.focus({ preventScroll: true });
      render();
    }
    function close() {
      audio.pause();
      overlay.hidden = true;
      if (cur >= 0) {
        itemOf(links[cur]).classList.remove(o.playingClass);
        links[cur].focus();
      }
    }
    const seekBy = (d) => { if (isFinite(audio.duration)) audio.currentTime = Math.max(0, Math.min(audio.duration, audio.currentTime + d)); };
    const volBy = (d) => { audio.volume = Math.max(0, Math.min(1, Math.round((audio.volume + d) * 16) / 16)); };
    const toggle = () => (audio.paused ? audio.play().catch(() => {}) : audio.pause());

    links.forEach((a, i) => a.addEventListener("click", (e) => {
      if (e.ctrlKey || e.metaKey || e.shiftKey || e.button) return;   // keep "open in new tab"
      e.preventDefault();
      open(i);
    }));

    for (const ev of ["timeupdate", "progress", "suspend", "waiting", "volumechange", "play", "pause", "durationchange", "loadedmetadata"]) {
      audio.addEventListener(ev, () => { if (!overlay.hidden) render(); });
    }
    audio.addEventListener("ended", () => { if (cur + 1 < links.length) open(cur + 1); });

    // The mouse works only on the two sliders (progress and volume).
    // The window is redrawn several times a second, so a "click" (down and up on the
    // same element) often never fires: act on pointerdown and keep dragging with the
    // slider geometry captured at that moment.
    let drag = null;                     // { act, left, width }
    const applyDrag = (x) => {
      const f = Math.min(1, Math.max(0, (x - drag.left) / drag.width));
      if (drag.act === "seek") {
        if (isFinite(audio.duration)) audio.currentTime = f * audio.duration;
      } else {
        audio.volume = Math.round(f * 16) / 16;
      }
    };
    overlay.addEventListener("pointerdown", (e) => {
      if (e.target === overlay) return close();
      const t = e.target.closest("[data-act]");
      if (!t || e.button !== 0) return;
      e.preventDefault();
      const r = t.getBoundingClientRect();
      drag = { act: t.dataset.act, left: r.left, width: r.width };
      overlay.setPointerCapture(e.pointerId);
      applyDrag(e.clientX);
    });
    overlay.addEventListener("pointermove", (e) => { if (drag) applyDrag(e.clientX); });
    const endDrag = () => { drag = null; };
    overlay.addEventListener("pointerup", endDrag);
    overlay.addEventListener("pointercancel", endDrag);

    doc.addEventListener("keydown", (e) => {
      if (overlay.hidden || e.ctrlKey || e.metaKey || e.altKey) return;
      // e.code, so the keys work in any keyboard layout
      const act = {
        Space: () => open(cur + 1), KeyP: () => open(cur - 1), KeyS: toggle, KeyA: specToggle, Escape: close, Backspace: close,
        Comma: () => seekBy(-10), Period: () => seekBy(10), ArrowLeft: () => seekBy(-10), ArrowRight: () => seekBy(10),
        ArrowUp: () => seekBy(60), ArrowDown: () => seekBy(-60),   // bigger steps, as in mpv
        Equal: () => volBy(1 / 16), NumpadAdd: () => volBy(1 / 16), Minus: () => volBy(-1 / 16), NumpadSubtract: () => volBy(-1 / 16),
      }[e.code];
      if (act) { e.preventDefault(); act(); }
    });

    return { open, close, audio };
  }

  global.TextPlayer = { attach, version: "0.1.0" };
})(typeof window !== "undefined" ? window : globalThis);
