# textplayer

**English** | [Русский](README.ru.md)

A DOS-style text-mode audio player for plain links to audio files. The window appears
centered over the page and is drawn entirely with characters: `╔═╗` frames, `█░` bars.
One file, no dependencies; it adds its own styles.

```
╔════════════════════ Browser Player v0.1 ═════════════════════╗
║ File:    mordax-bastards-zebra-2015-04-04-podkasty-04.04.15… ║
╟──────────────────────────────────────────────────────────────╢
║ Title:   ZEBRA                                               ║
║ Artist:  MORDAX Bastards                                     ║
║ Podcast: Zebra                                               ║
║ Date:    2015-04-04    Track: 1/25      Size: 86 MB          ║
╟──────────────────────────────────────────────────────────────╢
║ MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Stereo             ║
║ Decoder: Chrome    Vol: ██████████░░░░░░ -4dB                ║
║                                                              ║
║ ► 54:03 ████████████████████████████████████████░░░░░ 59:59  ║
║                                                              ║
║ Buf ███████████░ 94%                                         ║
╟──────────────────────────────────────────────────────────────╢
║ Spc:next P:prev <>:seek S:pause +-:vol Esc:exit              ║
╚══════════════════════════════════════════════════════════════╝
```

## Usage

```html
<ol>
  <li><a href="/media/a.mp3" data-title="ZEBRA" data-artist="MORDAX Bastards">…</a></li>
  <li><a href="/media/b.mp3">…</a></li>
</ol>
<script src="textplayer.js"></script>
<script>
  TextPlayer.attach({ links: "ol a[href$='.mp3']" });
</script>
```

The file can also be inlined into a `<script>` on the page. That is why `textplayer.js`
must never contain a closing script tag: the browser would end the script there.

If the repository is public, the file can be loaded from jsDelivr:
`https://cdn.jsdelivr.net/gh/<user>/<repo>@<tag>/web/textplayer/textplayer.js`
(pin a tag, not a branch).

Without JavaScript the links work as usual and open the file.
Ctrl-, Cmd- and Shift-click keep working too: the file opens in a new tab or window.

## Episode data

Taken from the link's `data-*` attributes. All of them are optional.

| attribute      | window line  | if missing                    |
|----------------|--------------|-------------------------------|
| `data-file`    | File         | file name from `href`         |
| `data-title`   | Title        | link text                     |
| `data-artist`  | Artist       | empty                         |
| `data-podcast` | Podcast      | empty                         |
| `data-date`    | Date         | empty                         |
| `data-size`    | Size (bytes) | empty                         |
| `data-format`  | stream line  | bitrate estimated from size and duration |

`data-format` is a ready-made string such as `MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Stereo`.
Browsers do not expose stream parameters, so they have to be extracted on the server
beforehand (in podcast-site this is done by `podscan`, module `mpeg.rs`).

## `attach` options

| option         | default                 | meaning |
|----------------|-------------------------|---------|
| `links`        | `"a[data-tp]"`          | CSS selector or an array of `<a>` elements |
| `title`        | `"Browser Player v0.1"` | title in the top frame |
| `item`         | `"li"`                  | closest ancestor of the link that gets `playingClass` (the link itself if there is none) |
| `playingClass` | `"tp-playing"`          | class of the current track, to highlight it in the list |
| `width`        | `62`                    | inner width of the window, in characters |

`attach` returns `{ open(index), close(), audio }`, or `null` if there are no links.

## Controls

- Keys: Space — next track, `P` — previous, `<` `>` or `←` `→` — seek 10 seconds,
  `S` — pause, `+` `-` — volume, `Esc` — close. Keys are matched by `e.code`, so they
  work in any keyboard layout.
- Mouse: click or drag on the progress and volume bars; a click outside the window closes it.
  The key help line is plain text and does not react to the mouse.

**Seeking requires the server to support Range requests (status 206).** Without them the
browser cannot jump to a position in the file.

## Styling

Colors and font are set with CSS variables on `:root` or `.tp-overlay`:

| variable          | default      | |
|-------------------|--------------|-|
| `--tp-bg`         | `#aaa`       | window background |
| `--tp-label`      | `#00a`       | labels and bars |
| `--tp-value`      | `#ff5`       | values |
| `--tp-frame`      | `#fff`       | frames |
| `--tp-title-bg`, `--tp-title` | `#fff`, `#555` | title |
| `--tp-help`       | `#000`       | key help |
| `--tp-backdrop`   | `rgb(0 0 0 / .55)` | page dimming |
| `--tp-font`       | `"DejaVu Sans Mono", Menlo, Consolas, "Liberation Mono", monospace` | font |
| `--tp-font-size`  | `min(16px, calc((100vw - 2rem) / 44))` | font size: the window fits a phone screen |
| `--tp-z`          | `1000`       | z-index |

The font must be monospaced and include box-drawing (`╔═║`) and block (`█░`) characters,
otherwise the frames fall apart.

## License

MIT.
