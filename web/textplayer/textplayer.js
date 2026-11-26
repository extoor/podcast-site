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
    width: 62,                           // inner width of the window, in columns
  };

  // Look of the window. Every color and the font can be changed with CSS variables
  // (--tp-*) on :root or on .tp-overlay, without touching this file.
  const CSS = `
.tp-overlay { position: fixed; inset: 0; z-index: var(--tp-z, 1000); display: flex;
  justify-content: center; align-items: center; padding: 1rem; background: var(--tp-backdrop, rgb(0 0 0 / .55)); }
/* author display:flex would otherwise override the hidden attribute */
.tp-overlay[hidden] { display: none; }
.tp-win { margin: 0; padding: .5lh 1ch; background: var(--tp-bg, #aaa); color: var(--tp-label, #00a);
  user-select: none; font: bold var(--tp-font-size, min(16px, calc((100vw - 2rem) / 44))) / normal
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
    const sep = () => `<span class="tp-fr">╟${"─".repeat(W)}╢</span>`;
    const L = (s) => seg(s, "tp-lb"), V = (s) => seg(s, "tp-va");
    // Key help is plain text: keyboard only, the mouse does nothing here.
    const key = (k, label) => seg(`${k}:${label} `, "tp-help");
    const itemOf = (a) => (o.item && a.closest(o.item)) || a;

    function render() {
      const link = links[cur], a = link.dataset;
      const dur = audio.duration, size = Number(a.size);
      const kbps = size > 0 && isFinite(dur) && dur > 0 ? Math.round((size * 8) / dur / 1000) : null;
      const db = audio.volume > 0 ? Math.round(20 * Math.log10(audio.volume)) + "dB" : "mute";
      let loaded = 0;
      for (let i = 0; i < audio.buffered.length; i++) loaded = Math.max(loaded, audio.buffered.end(i));
      const buf = isFinite(dur) && dur > 0 ? loaded / dur : 0;

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
        row(L("Decoder: "), V(BROWSER.padEnd(10)), L("Vol: "),
            seg(bar(audio.volume, 16), "tp-lb tp-slider", 'data-act="vol"'), V(" " + db)),
        row(),
        row(seg(audio.paused ? "║" : "►", "tp-lb tp-st"), L(" " + ct + " "),
            seg(bar(audio.currentTime / dur, pw), "tp-lb tp-slider", 'data-act="seek"'), L(" " + dt)),
        row(),
        row(L("Buf "), seg(bar(buf, 12), "tp-lb"), V(" " + Math.round(buf * 100) + "%")),
        sep(),
        row(key("Spc", "next"), key("P", "prev"), key("<>", "seek"), key("S", "pause"), key("+-", "vol"), key("Esc", "exit")),
        `<span class="tp-fr">╚${"═".repeat(W)}╝</span>`,
      ].join("\n");
    }

    function open(i) {
      if (i < 0 || i >= links.length) return;
      if (cur >= 0) itemOf(links[cur]).classList.remove(o.playingClass);
      cur = i;
      itemOf(links[cur]).classList.add(o.playingClass);
      audio.src = links[cur].href;
      audio.play().catch(() => {});
      overlay.hidden = false;
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

    for (const ev of ["timeupdate", "progress", "volumechange", "play", "pause", "durationchange", "loadedmetadata"]) {
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
        Space: () => open(cur + 1), KeyP: () => open(cur - 1), KeyS: toggle, Escape: close,
        Comma: () => seekBy(-10), Period: () => seekBy(10), ArrowLeft: () => seekBy(-10), ArrowRight: () => seekBy(10),
        Equal: () => volBy(1 / 16), NumpadAdd: () => volBy(1 / 16), Minus: () => volBy(-1 / 16), NumpadSubtract: () => volBy(-1 / 16),
      }[e.code];
      if (act) { e.preventDefault(); act(); }
    });

    return { open, close, audio };
  }

  global.TextPlayer = { attach, version: "0.1.0" };
})(typeof window !== "undefined" ? window : globalThis);
