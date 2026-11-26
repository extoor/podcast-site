/*!
 * keynav — keyboard navigation over a list of links, terminal style.
 *   ↑ ↓ (and Home / End) move the selection, Enter opens the selected link
 *   (it is a real focused link, so Enter is a native click), Backspace goes back.
 * No dependencies. MIT License.
 *
 * Usage: load this file, then call
 *   KeyNav.attach({ items: "ul li > a:first-child", back: "/" });
 *
 * This file may be inlined into a page: it must never contain a closing script tag.
 */
(function (global) {
  "use strict";

  /**
   * options:
   *   items  — CSS selector of the links to move through (in document order);
   *   back   — URL to open on Backspace (omit on the top-level page);
   *   paused — function: true while another widget owns the keyboard
   *            (default: a visible textplayer window).
   */
  function attach(options) {
    const doc = global.document;
    const o = Object.assign({
      items: "a",
      back: null,
      paused: () => !!doc.querySelector(".tp-overlay:not([hidden])"),
    }, options);
    const items = () => [...doc.querySelectorAll(o.items)];

    const select = (a) => {
      if (!a) return;
      a.focus({ preventScroll: true });
      a.scrollIntoView({ block: "nearest" });
    };

    // Coming back from a sub-page: select the link that led there.
    try {
      const from = doc.referrer && new URL(doc.referrer);
      if (from && from.origin === global.location.origin && from.pathname !== global.location.pathname) {
        select(items().find((a) => new URL(a.href, global.location.href).pathname === from.pathname));
      }
    } catch { /* no usable referrer */ }

    doc.addEventListener("keydown", (e) => {
      if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented || o.paused()) return;
      const t = e.target;
      if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;

      const list = items();
      const i = list.indexOf(doc.activeElement);
      let next;
      switch (e.key) {
        case "ArrowDown": next = i < 0 ? list[0] : list[Math.min(i + 1, list.length - 1)]; break;
        case "ArrowUp": next = i < 0 ? list[list.length - 1] : list[Math.max(i - 1, 0)]; break;
        case "Home": next = list[0]; break;
        case "End": next = list[list.length - 1]; break;
        case "Backspace":
          if (!o.back) return;
          e.preventDefault();
          global.location.href = o.back;
          return;
        default:
          return;   // Enter: the focused link handles it natively
      }
      e.preventDefault();
      select(next);
    });
  }

  global.KeyNav = { attach, version: "0.1.0" };
})(typeof window !== "undefined" ? window : globalThis);
