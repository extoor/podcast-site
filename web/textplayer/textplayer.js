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

  // Semantic version of this module, its only copy: the title and TextPlayer.version
  // show it. Bump it with every change and add a CHANGELOG.md entry (CI checks).
  const VERSION = "0.4.0";

  const DEFAULTS = {
    links: "a[data-tp]",                 // CSS selector or an array of <a> elements
    title: `Browser Player v${VERSION}`, // shown in the top frame
    lang: "",                            // help language (en, ru, uk); default: <html lang>
    item: "li",                          // closest element that gets `playingClass`
    playingClass: "tp-playing",
    width: 70,                           // inner width of the window, in columns
    bufferTarget: 60,                    // seconds buffered ahead that count as a full "Buf"
    spectrum: true,                      // spectrum analyzer under "Buf" (same-origin audio only)
    spectrumRows: 16,                    // segments per bar (a segment is about a third of a line)
    spectrumCookie: "tp_spectrum",       // cookie that keeps the A key choice (shown / hidden)
    spectrumTheme: "classic",            // analyzer colors: an id from TextPlayer.themes
    spectrumThemeCookie: "tp_spectrum_theme", // cookie that keeps the T key choice (theme id)
    engine: null,                        // { wasm, worklet }: URLs of the WebAssembly engine
    gapless: true,                       // with the engine: the next track follows without a gap
    gaplessCookie: "tp_gapless",         // cookie that keeps the choice in the options (O key)
  };

  // The H key help, in the page's language. Each row: the keys, what they do (wrapped
  // to the window). The short key line at the bottom stays English, like the window.
  const HELP = {
    en: {
      title: "Keys",
      rows: [
        ["Space", "next track"],
        ["P", "previous track"],
        ["← →  < >", "seek 10 seconds back / forward"],
        ["↑ ↓", "seek a minute forward / back"],
        ["S", "pause / resume"],
        ["+ -", "volume up / down"],
        ["A", "show / hide the spectrum analyzer"],
        ["T  Shift+T", "next / previous color theme of the analyzer"],
        ["O", "options: gapless playback, the analyzer and its theme; O or Esc closes them"],
        ["H", "this help; H or Esc closes it"],
        ["Esc  Bksp", "close the player"],
        ["Mouse", "click or drag the progress and volume bars; a click outside the window closes it"],
      ],
    },
    ru: {
      title: "Клавиши",
      rows: [
        ["Пробел", "следующий трек"],
        ["P", "предыдущий трек"],
        ["← →  < >", "перемотка на 10 секунд назад / вперёд"],
        ["↑ ↓", "перемотка на минуту вперёд / назад"],
        ["S", "пауза / продолжить"],
        ["+ -", "громче / тише"],
        ["A", "показать / спрятать анализатор спектра"],
        ["T  Shift+T", "следующая / предыдущая цветовая тема анализатора"],
        ["O", "настройки: бесшовное воспроизведение, анализатор и его тема; закрыть — O или Esc"],
        ["H", "эта справка; закрыть — H или Esc"],
        ["Esc  Bksp", "закрыть плеер"],
        ["Мышь", "клик и перетаскивание по полосам прогресса и громкости; клик мимо окна закрывает его"],
      ],
    },
    uk: {
      title: "Клавіші",
      rows: [
        ["Пробіл", "наступний трек"],
        ["P", "попередній трек"],
        ["← →  < >", "перемотка на 10 секунд назад / вперед"],
        ["↑ ↓", "перемотка на хвилину вперед / назад"],
        ["S", "пауза / продовжити"],
        ["+ -", "гучніше / тихіше"],
        ["A", "показати / сховати аналізатор спектра"],
        ["T  Shift+T", "наступна / попередня кольорова тема аналізатора"],
        ["O", "налаштування: безшовне відтворення, аналізатор і його тема; закрити — O або Esc"],
        ["H", "ця довідка; закрити — H або Esc"],
        ["Esc  Bksp", "закрити плеєр"],
        ["Миша", "клік і перетягування по смугах прогресу та гучності; клік поза вікном закриває його"],
      ],
    },
  };
  HELP.ua = HELP.uk;                    // the country code is a common mistake for the language
  // The O key options, in the page's language.
  const OPTIONS = {
    en: { title: "Options", gapless: "Gapless playback", spectrum: "Spectrum analyzer", theme: "Analyzer theme",
      on: "on", off: "off", shown: "shown", hidden: "hidden", na: "not available",
      keys: "↑↓ select   ←→ Space change   O Esc close" },
    ru: { title: "Настройки", gapless: "Бесшовное воспроизведение", spectrum: "Анализатор спектра", theme: "Тема анализатора",
      on: "вкл", off: "выкл", shown: "показан", hidden: "скрыт", na: "недоступно",
      keys: "↑↓ выбор   ←→ пробел изменить   O Esc закрыть" },
    uk: { title: "Налаштування", gapless: "Безшовне відтворення", spectrum: "Аналізатор спектра", theme: "Тема аналізатора",
      on: "увімк", off: "вимк", shown: "показано", hidden: "приховано", na: "недоступно",
      keys: "↑↓ вибір   ←→ пробіл змінити   O Esc закрити" },
  };
  OPTIONS.ua = OPTIONS.uk;
  // Words of `text` in lines of at most `n` characters.
  const wrap = (text, n) => text.split(" ").reduce((lines, w) => {
    const last = lines[lines.length - 1];
    if (last && (last + " " + w).length <= n) lines[lines.length - 1] = last + " " + w;
    else lines.push(w.length > n ? fit(w, n) : w);
    return lines;
  }, []);

  // Color themes of the spectrum analyzer, switched with T (Shift+T back). A theme
  // colors each line of segments by its height f (0 at the bottom .. 1 at the top):
  // either stepped bands or a gradient through the given colors, bottom to top. The
  // window's gray background is the same in all of them, so no near-white tops.
  const gradient = (...stops) => (f) => {
    const rgb = stops.map((h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)));
    const x = f * (rgb.length - 1), i = Math.min(Math.floor(x), rgb.length - 2), k = x - i;
    return "#" + rgb[i].map((a, j) => Math.round(a + (rgb[i + 1][j] - a) * k)
      .toString(16).padStart(2, "0")).join("");
  };
  const THEMES = [
    // the LED meter of hi-fi decks and Winamp: green, yellow, red (CSS variables)
    { id: "classic", name: "Classic", color: (f) => f > 0.8 ? "var(--tp-spec-high, #d22)"
      : f > 0.5 ? "var(--tp-spec-mid, #cc0)" : "var(--tp-spec-low, #1a1)" },
    { id: "fire", name: "Fire", color: gradient("#770000", "#cc0000", "#ff4400", "#ff9900", "#ffee00") },
    { id: "ice", name: "Ice", color: gradient("#001155", "#0044aa", "#0088dd", "#00bbff", "#00eeee") },
    { id: "rainbow", name: "Rainbow", color: (f) => `hsl(${Math.round(270 * (1 - f))} 90% 45%)` },
    { id: "matrix", name: "Matrix", color: gradient("#003300", "#006600", "#00aa00", "#00dd00", "#22ff22") },
    { id: "amber", name: "Amber", color: gradient("#663300", "#994400", "#cc7700", "#ffaa00", "#ffcc00") },
    { id: "synthwave", name: "Synthwave", color: gradient("#330088", "#7700cc", "#cc00cc", "#ff0088", "#00dddd") },
  ];

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
  // --- WebAssembly engine ---------------------------------------------------------
  // With options.engine ({ wasm, worklet }: URLs of audiocore's module and worklet.js),
  // MP3 plays through Web Audio instead of an <audio> element: the page downloads the
  // file (paced: about two minutes ahead), the worklet decodes it with audiocore. That
  // makes gapless playback possible: the next track is fetched ahead and starts on the
  // very next sample. The object below stands in for the <audio> element: the same
  // properties, methods and events the player uses.
  const PACE = 120;                     // seconds of compressed audio kept ahead
  const CHUNK = 512 * 1024;             // bytes per range request: about 20 s at 192 kbps
  function wasmAudio(cfg) {             // cfg: { wasm, worklet, gapless, next(), fail() }
    const ev = new EventTarget();
    const fire = (type) => ev.dispatchEvent(new Event(type));
    const AC = global.AudioContext || global.webkitAudioContext;
    let ctx;
    // the library is 44.1 kHz: a context at that rate needs no resampling in the worklet
    try { ctx = new AC({ sampleRate: 44100, latencyHint: "playback" }); } catch { ctx = new AC(); }
    const gain = ctx.createGain();
    gain.connect(ctx.destination);
    let node = null, port = null, dead = false;
    const queue = [];                    // messages sent before the worklet is up
    const post = (m, transfer = []) => (port ? port.postMessage(m, transfer) : queue.push([m, transfer]));
    const taps = [];                     // analysers waiting for the node
    const ready = (async () => {
      const res = await fetch(cfg.wasm);
      if (!res.ok) throw new Error(`${cfg.wasm}: ${res.status}`);
      const module = await WebAssembly.compile(await res.arrayBuffer());
      await ctx.audioWorklet.addModule(cfg.worklet);
      node = new global.AudioWorkletNode(ctx, "textplayer", {
        numberOfInputs: 0, outputChannelCount: [2], processorOptions: { module, gapless: cfg.gapless },
      });
      node.connect(gain);
      taps.splice(0).forEach((an) => node.connect(an));
      port = node.port;
      port.onmessage = (e) => message(e.data);
      for (const [m, t] of queue.splice(0)) port.postMessage(m, t);
    })();
    ready.catch(() => { if (!dead) { api.destroy(); cfg.fail(); } });

    // A track: one download of one file into one worklet stream. A seek is a new track
    // of the same file that starts at `base` seconds; it keeps the file's format.
    let seq = 0, cur = null, next = null, paused = true, ended = false;
    let ticks = [];                      // downloads waiting for the next position report
    const tick = () => new Promise((r) => ticks.push(r));
    const byId = (id) => (cur && cur.id === id ? cur : next && next.id === id ? next : null);
    const duration = (t) => !t ? NaN : t.total > 0 ? t.total / t.rate
      : t.bitrate && t.size ? (t.size - t.start) * 8 / (t.bitrate * 1000) : NaN;
    const ahead = (t) => (t.bitrate ? t.pending / (t.bitrate * 125) : t.pending / 2e5);

    function track(url, base, from) {
      const t = { id: ++seq, url, base, frames: 0, pending: 0, received: 0, done: false,
        rate: 0, total: -1, bitrate: 0, start: 0, padding: 0, size: 0, ctrl: null };
      if (from) Object.assign(t, { rate: from.rate, total: from.total, bitrate: from.bitrate,
        start: from.start, padding: from.padding, size: from.size });
      return t;
    }
    function stop(t) {
      if (!t) return;
      if (t.ctrl) t.ctrl.abort();
      post({ t: "drop", id: t.id });
    }
    // Downloads `t` from byte `offset` in chunks: one short range request each, made only
    // when the worklet runs low (PACE ahead). No connection is held open for minutes: a
    // paced long download got cut somewhere on the way (a proxy, a port forward) and the
    // player stalled. A failed request is retried with a growing delay while the track
    // is still wanted.
    async function pump(t, offset) {
      const ctrl = t.ctrl = new AbortController();
      let pos = offset, tries = 0;
      // hands bytes to the worklet; returns how many (the buffer is transferred: empty after)
      const give = (buf) => {
        const n = buf.byteLength;
        pos += n;
        t.received += n;
        t.pending += n;
        post({ t: "data", id: t.id, buf }, [buf]);
        if (t === cur) fire("progress");
        return n;
      };
      while (!ctrl.signal.aborted) {
        while (ahead(t) > (t.bitrate ? PACE : 1) && !ctrl.signal.aborted) await tick();
        if (ctrl.signal.aborted) return;
        try {
          const res = await fetch(t.url, { signal: ctrl.signal, headers: { Range: `bytes=${pos}-${pos + CHUNK - 1}` } });
          if (res.status === 416) break;                  // asked past the end
          if (!res.ok) throw new Error(`HTTP ${res.status}`);
          if (res.status !== 206) {
            // no range support: the whole file in one answer, usable only from the start
            if (pos) throw new Error("the server ignores Range");
            t.base = 0;
            give(await res.arrayBuffer());
            break;
          }
          const range = /\/(\d+)$/.exec(res.headers.get("Content-Range") || "");
          if (range) t.size = Number(range[1]);
          const n = give(await res.arrayBuffer());
          tries = 0;
          if (!n || (t.size && pos >= t.size)) break;
        } catch (e) {
          if (ctrl.signal.aborted) return;
          global.console.warn(`textplayer: ${t.url} from byte ${pos}: ${e.message}; trying again`);
          if (t === cur) fire("waiting");
          await new Promise((r) => global.setTimeout(r, Math.min(15000, 1000 * 2 ** tries++)));
        }
      }
      if (ctrl.signal.aborted) return;
      t.done = true;
      post({ t: "end", id: t.id });
      if (t === cur) { fire("progress"); preload(); }
    }
    // gapless: once the current file is downloaded, fetch the next one ahead
    function preload() {
      if (!cfg.gapless || next || !cur || !cur.done) return;
      const url = cfg.next();
      if (!url) return;
      next = track(url, 0);
      post({ t: "open", id: next.id, trim: 0 });
      post({ t: "next", id: next.id });
      pump(next, 0);
    }
    function start(url, at) {
      ended = false;
      if (!at && next && next.url === url) {           // already on its way: just switch
        stop(cur);
        cur = next;
        next = null;
        post({ t: "play", id: cur.id });
      } else {
        const from = cur && cur.url === url && cur.rate ? cur : null;
        stop(cur);
        stop(next);
        next = null;
        cur = track(url, at, from);
        // a seek: the byte for that time (constant bitrate), and the end cut kept exact
        const offset = at && from ? Math.floor(from.start + (at / duration(from)) * (from.size - from.start)) : 0;
        post({ t: "open", id: cur.id, trim: offset ? from.padding : 0 });
        post({ t: "play", id: cur.id });
        pump(cur, offset);
      }
      fire("durationchange");
      fire("timeupdate");
    }
    function message(m) {
      switch (m.t) {
        case "format": {
          const t = byId(m.id);
          if (t && !t.rate) {
            Object.assign(t, { rate: m.rate, total: m.total, bitrate: m.bitrate, start: m.start, padding: m.padding });
            if (t === cur) { fire("durationchange"); fire("loadedmetadata"); }
          }
          break;
        }
        case "time":
          if (cur && m.id === cur.id) { cur.frames = m.frames; cur.pending = m.pending; }
          if (next && m.nextId === next.id) next.pending = m.nextPending;
          ticks.splice(0).forEach((r) => r());
          fire("timeupdate");
          break;
        case "waiting":
          fire("waiting");
          break;
        case "ended":
          if (!cur || m.id !== cur.id) break;
          if (m.next !== null && next && m.next === next.id) {
            cur = next;                                   // went on without a gap
            next = null;
            fire("advance");
            fire("durationchange");
            preload();
          } else {
            cur.frames = Math.round(((duration(cur) || 0) - cur.base) * ctx.sampleRate);
            ended = true;
            fire("ended");
          }
          break;
      }
    }

    const api = {
      NETWORK_IDLE: 1, HAVE_FUTURE_DATA: 3, preload: "auto",
      ctx,
      get src() { return cur ? cur.url : ""; },
      set src(url) { start(url, 0); },
      get paused() { return paused; },
      get ended() { return ended; },
      get duration() { return duration(cur); },
      get currentTime() { return cur ? cur.base + cur.frames / ctx.sampleRate : 0; },
      set currentTime(t) {
        const d = duration(cur);
        if (cur && isFinite(d)) start(cur.url, Math.max(0, Math.min(d - 0.05, t)));
      },
      get volume() { return gain.gain.value; },
      set volume(v) { gain.gain.value = v; fire("volumechange"); },
      get buffered() {
        const t = cur, end = t ? (t.done ? duration(t) : t.base + (t.bitrate ? t.received / (t.bitrate * 125) : 0)) : 0;
        return { length: t ? 1 : 0, start: () => (t ? t.base : 0), end: () => end || 0 };
      },
      get networkState() { return cur && !cur.done ? 2 : 1; },
      get readyState() { return cur && cur.pending > 0 ? 4 : 0; },
      get gapless() { return cfg.gapless; },
      set gapless(on) {
        cfg.gapless = !!on;
        post({ t: "gapless", on: cfg.gapless });
        if (on) preload(); else { stop(next); next = null; }
      },
      play() {
        if (ended && cur) start(cur.url, 0);
        paused = false;
        post({ t: "resume" });
        fire("play");
        return Promise.all([ctx.resume(), ready]).then(() => {});
      },
      pause() {
        paused = true;
        post({ t: "pause" });
        fire("pause");
      },
      // the analyser listens to the worklet's output (before the volume)
      tap(an) { if (node) node.connect(an); else taps.push(an); },
      destroy() {
        dead = true;
        stop(cur);
        stop(next);
        cur = next = null;
        ctx.close().catch(() => {});
      },
      addEventListener: (...a) => ev.addEventListener(...a),
      removeEventListener: (...a) => ev.removeEventListener(...a),
    };
    post({ t: "pause" });
    return api;
  }

  function attach(options) {
    const o = Object.assign({}, DEFAULTS, options);
    const doc = global.document;
    const links = typeof o.links === "string" ? [...doc.querySelectorAll(o.links)] : [...o.links];
    if (!links.length) return null;
    injectStyle(doc);

    const W = o.width;
    const lang = (o.lang || doc.documentElement.lang || "en").toLowerCase().split("-")[0];
    const help = HELP[lang] || HELP.en;
    let helpShown = false;                 // the H key: the help instead of the key line
    let optsShown = false, optsAt = 0;     // the O key: the options instead of it, the row picked
    const BROWSER = browserName();
    let cur = -1;
    // Audio from another origin (without CORS) can't be fetched, nor read by Web Audio.
    const sameOrigin = links.every((a) => {
      try { return new URL(a.href, global.location.href).origin === global.location.origin; }
      catch { return false; }
    });
    // The WebAssembly engine when it is given and can run; else an <audio> element. If the
    // engine fails to load (old browser, blocked file), the element takes over.
    const canEngine = o.engine && o.engine.wasm && o.engine.worklet && sameOrigin
      && global.AudioWorkletNode && global.WebAssembly && links.every((a) => /\.mp3$/i.test(new URL(a.href, global.location.href).pathname));
    let audio = canEngine ? wasmAudio({
      // the visitor's choice from the options (cookie) over the page's default
      wasm: o.engine.wasm, worklet: o.engine.worklet,
      gapless: readCookie(o.gaplessCookie) ? readCookie(o.gaplessCookie) === "1" : o.gapless,
      next: () => (cur + 1 < links.length ? links[cur + 1].href : null),
      fail: () => {
        audio = new Audio();
        bind(audio);
        if (spec.an) Object.assign(spec, { an: null, ctx: null });   // rebuilt for the element
        if (cur >= 0 && !overlay.hidden) open(cur);
      },
    }) : new Audio();
    audio.preload = "auto";

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
    // A separator with labels: `center` in the middle (the open panel's title), `right`
    // near the right end (a theme's name): ╟──── Options ──────── Fire ─╢. Without
    // room for both, the right one is left out.
    const sepWith = (center, right) => {
      const parts = [];
      if (center) parts.push([Math.floor((W - center.length - 2) / 2), center]);
      if (right) {
        const at = W - right.length - 3;
        if (!parts.length || at > parts[0][0] + parts[0][1].length + 2) parts.push([at, right]);
      }
      let out = `<span class="tp-fr">╟`, x = 0;
      for (const [at, text] of parts) {
        out += `${"─".repeat(at - x)} </span><span class="tp-help">${esc(text)}</span><span class="tp-fr"> `;
        x = at + text.length + 2;
      }
      return `${out}${"─".repeat(W - x)}╢</span>`;
    };
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
        row(L("Codec:   "), V(fit(a.format || (kbps ? `${kbps} kbps` : ""), W - 11))),
        // decoder left, buffer (time buffered ahead, as on the progress line) in the
        // middle, volume right: the free columns are shared out between them
        row(...spread(
          // who decodes: the WebAssembly engine, or the browser behind <audio>
          [L("Decoder: "), V(audio.tap ? "audiocore" : BROWSER)],
          [L("Buf: "), seg(bar(buf, 13), "tp-lb"), V(" " + time(ahead))],
          [L("Vol: "), seg(bar(audio.volume, 13), "tp-lb tp-slider", 'data-act="vol"'), V(" " + db.padStart(5))],
        )),
        // the playback section: progress (and the analyzer), between two separators
        sep(),
        row(seg(audio.paused ? "║" : "►", "tp-lb tp-st"), L(" " + ct + " "),
            seg(bar(audio.currentTime / dur, pw), "tp-lb tp-slider", 'data-act="seek"'), L(" " + dt)),
        // the analyzer belongs to the playback section: no separator above it
        ...(spec.on && spec.shown ? [`<span class="tp-specbox">${Array.from({ length: spec.rows }, (_, r) =>
          `<span class="tp-fr">║</span> <span class="tp-spec">${spec.line(r)}</span> <span class="tp-fr">║</span>`).join("\n")}</span>`] : []),
        // above the help or the options their title; for a second after T, the theme's name
        sepWith(helpShown ? help.title : optsShown ? optsText.title : "",
          spec.on && spec.shown && spec.named ? THEMES[spec.theme].name : ""),
        // Esc is in the help: the line has no room left for it next to O
        ...(helpShown ? helpRows() : optsShown ? optsRows() : [row(...keys(["Spc", "next"], ["P", "prev"],
          ["←→↑↓", "seek"], ["S", "pause"], ["+-", "vol"], ["O", "opts"], ["H", "help"]))]),
        `<span class="tp-fr">╚${"═".repeat(W)}╝</span>`,
      ].join("\n");
    }

    // --- spectrum analyzer ----------------------------------------------------
    // Web Audio: the element feeds an AnalyserNode. Routing an element through Web
    // Audio is for good, and audio from another origin (without CORS) would then come
    // out silent, so it is used only when every link is on this site.
    const spec = {
      on: o.spectrum && sameOrigin && !!(global.AudioContext || global.webkitAudioContext),
      shown: readCookie(o.spectrumCookie) !== "0",   // the A key; shown unless turned off
      // the T key: the remembered theme, else the option's, else the first one
      theme: [readCookie(o.spectrumThemeCookie), o.spectrumTheme]
        .map((id) => THEMES.findIndex((t) => t.id === id)).find((i) => i >= 0) ?? 0,
      rows: o.spectrumRows,
      // a bar is 2 columns, bars 1 apart: as many as fit the inner width (23 at 70),
      // so the spectrum lines up with the text above it, edge to edge
      bands: Math.floor((W - 1) / 3),
      named: 0,                             // timer while the theme's name is shown
      levels: null,                         // per band, 0..rows
      peaks: null, held: null,              // peak level per band, when it was reached
      ctx: null, an: null, data: null, raf: 0, last: 0,
      // one text line of the bars, row r from the top: colored "▂▂" segments
      line(r) {
        const t = this.rows - r;            // the level this line lights at
        const color = THEMES[this.theme].color(t / this.rows);
        const cells = [];
        for (let b = 0; b < this.bands; b++) {
          const level = this.levels ? Math.round(this.levels[b]) : 0;
          const peak = this.peaks ? Math.round(this.peaks[b]) : 0;
          // a bar up to its level, and above it the hanging peak segment
          cells.push(level >= t || (peak === t && peak > level) ? "▂▂" : "  ");
        }
        // bars centered in the inner width: the spare columns split left and right
        const bars = cells.join(" "), left = Math.floor((W - 2 - bars.length) / 2);
        return `<span style="color:${color}">${(" ".repeat(left) + bars).padEnd(W - 2)}</span>`;
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
    // T / Shift+T: the next / previous color theme, remembered in a cookie for a year.
    function specTheme(back) {
      if (!spec.on) return;
      spec.theme = (spec.theme + (back ? THEMES.length - 1 : 1)) % THEMES.length;
      writeCookie(o.spectrumThemeCookie, THEMES[spec.theme].id);
      // the name for a second; another T starts the second again
      global.clearTimeout(spec.named);
      spec.named = global.setTimeout(() => { spec.named = 0; render(); }, 1000);
      if (!spec.shown) specToggle();
      else render();
    }
    function specStart() {
      if (!spec.on || !spec.shown) return;
      if (!spec.an) {
        try {
          const AC = global.AudioContext || global.webkitAudioContext;
          // the engine has its own context and taps its output; an element is routed
          // through the analyser to the speakers
          spec.ctx = audio.ctx || new AC();
          spec.an = spec.ctx.createAnalyser();
          spec.an.fftSize = 4096;
          spec.an.smoothingTimeConstant = 0.5;
          if (audio.tap) audio.tap(spec.an);
          else {
            spec.ctx.createMediaElementSource(audio).connect(spec.an);
            spec.an.connect(spec.ctx.destination);
          }
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
    // The help: a title line, then each key and what it does, the description wrapped
    // under itself. Without an analyzer its keys are left out.
    function helpRows() {
      const kw = 12, tw = W - 2 - kw;
      const lines = [];                                       // the title is in the separator
      for (const [k, text] of help.rows) {
        if (!spec.on && /^[AT]\b/.test(k)) continue;
        wrap(text, tw).forEach((t, i) => lines.push(row(V((i ? "" : k).padEnd(kw)), seg(t, "tp-help"))));
      }
      return lines;
    }
    function helpToggle(on = !helpShown) {
      helpShown = on;
      if (on) optsShown = false;
      render();
    }
    // The options: each with its current value; ←→ (or Space, Enter) change the one
    // picked with ↑↓. Every change is kept in a cookie, like the A and T keys.
    const optsText = OPTIONS[lang] || OPTIONS.en;
    const optsList = () => [
      { label: optsText.gapless, can: () => "gapless" in audio,
        value: () => (audio.gapless ? `[■] ${optsText.on}` : `[ ] ${optsText.off}`),
        change: () => {
          audio.gapless = !audio.gapless;
          writeCookie(o.gaplessCookie, audio.gapless ? "1" : "0");
        } },
      { label: optsText.spectrum, can: () => spec.on,
        value: () => (spec.shown ? `[■] ${optsText.shown}` : `[ ] ${optsText.hidden}`),
        change: () => specToggle() },
      { label: optsText.theme, can: () => spec.on,
        value: () => `‹ ${THEMES[spec.theme].name} ›`,
        change: (back) => specTheme(back) },
    ];
    function optsRows() {
      const list = optsList(), lw = Math.min(40, Math.max(...list.map((x) => x.label.length)));
      const lines = [];
      list.forEach((x, i) => {
        // the picked row: a marker and the label highlighted; the value three columns on
        const mark = i === optsAt ? "► " : "  ";
        lines.push(row(seg(mark, "tp-va"), seg(fit(x.label, lw), i === optsAt ? "tp-ttl" : "tp-lb"),
          seg(" ".repeat(lw - Math.min(lw, x.label.length) + 3)), V(x.can() ? x.value() : optsText.na)));
      });
      lines.push(row(seg("")), row(seg(optsText.keys, "tp-help")));   // the keys, a line apart
      return lines;
    }
    function optsToggle(on = !optsShown) {
      optsShown = on;
      if (on) helpShown = false;
      render();
    }
    // a key while the options are open: true if it was theirs
    function optsKey(code, shift) {
      const list = optsList(), x = list[optsAt];
      if (code === "ArrowUp" || code === "ArrowDown") {
        optsAt = (optsAt + (code === "ArrowUp" ? list.length - 1 : 1)) % list.length;
      } else if (["ArrowLeft", "ArrowRight", "Space", "Enter", "NumpadEnter"].includes(code)) {
        if (x.can()) x.change(code === "ArrowLeft" || shift);
      } else {
        return false;
      }
      render();
      return true;
    }
    function close() {
      helpShown = false;
      optsShown = false;
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

    function bind(a) {
      for (const ev of ["timeupdate", "progress", "suspend", "waiting", "volumechange", "play", "pause", "durationchange", "loadedmetadata"]) {
        a.addEventListener(ev, () => { if (a === audio && !overlay.hidden) render(); });
      }
      a.addEventListener("ended", () => { if (a === audio && cur + 1 < links.length) open(cur + 1); });
      // the engine went on to the next track by itself (gapless): follow it in the list
      a.addEventListener("advance", () => {
        if (a !== audio || cur + 1 >= links.length) return;
        itemOf(links[cur]).classList.remove(o.playingClass);
        cur += 1;
        itemOf(links[cur]).classList.add(o.playingClass);
        if (!overlay.hidden) render();
      });
    }
    bind(audio);

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
      if (optsShown && optsKey(e.code, e.shiftKey)) { e.preventDefault(); return; }
      // Esc / Backspace close an open panel (help, options) first, then the player
      const back = () => (helpShown ? helpToggle(false) : optsShown ? optsToggle(false) : close());
      const act = {
        Space: () => open(cur + 1), KeyP: () => open(cur - 1), KeyS: toggle, KeyA: specToggle, KeyT: () => specTheme(e.shiftKey), KeyH: () => helpToggle(),
        KeyO: () => optsToggle(), Escape: back, Backspace: back,
        Comma: () => seekBy(-10), Period: () => seekBy(10), ArrowLeft: () => seekBy(-10), ArrowRight: () => seekBy(10),
        ArrowUp: () => seekBy(60), ArrowDown: () => seekBy(-60),   // bigger steps, as in mpv
        Equal: () => volBy(1 / 16), NumpadAdd: () => volBy(1 / 16), Minus: () => volBy(-1 / 16), NumpadSubtract: () => volBy(-1 / 16),
      }[e.code];
      if (act) { e.preventDefault(); act(); }
    });

    return { open, close, audio };
  }

  global.TextPlayer = { attach, themes: THEMES.map(({ id, name }) => ({ id, name })), version: VERSION };
})(typeof window !== "undefined" ? window : globalThis);
