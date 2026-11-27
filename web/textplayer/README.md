# textplayer

**English** | [Русский](README.ru.md)

A DOS-style text-mode audio player for plain links to audio files. The window appears
centered over the page and is drawn entirely with characters: `╔═╗` frames, `█░` bars.
One file, no dependencies; it adds its own styles.

```
╔════════════════════════ Browser Player v0.1 ═════════════════════════╗
║ File:    mordax-bastards-zebra-2015-04-04-podkasty-04.04.15-kissfm.… ║
╟──────────────────────────────────────────────────────────────────────╢
║ Title:   ZEBRA                                                       ║
║ Artist:  MORDAX Bastards                                             ║
║ Podcast: Zebra                                                       ║
║ Date:    04.04.2015    Track: 1/25      Size: 86 MB                  ║
╟──────────────────────────────────────────────────────────────────────╢
║ Type:    MPEG-1 Layer III  192 kbps CBR  44.1 kHz  Stereo            ║
║ Decoder: Chrome   Buf: █████████████ 02:19  Vol: █████████████   0dB ║
╟──────────────────────────────────────────────────────────────────────╢
║ ► 00:02 ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░ 59:59 ║
╟──────────────────────────────────────────────────────────────────────╢
║ Spc:next | P:prev | ←→↑↓:seek | S:pause | +-:vol | A:spec | Esc:exit ║
╚══════════════════════════════════════════════════════════════════════╝
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
| `data-format`  | Type         | bitrate estimated from size and duration |

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
| `width`        | `70`                    | inner width of the window, in characters |
| `bufferTarget` | `60`                    | seconds buffered ahead of the playhead shown as a full `Buf` bar |
| `spectrum`     | `true`                  | spectrum analyzer under the progress line: LED-style bars of `▂▂` segments, from 50 Hz to 16 kHz (Web Audio) |
| `spectrumRows` | `16`                    | segments per bar (a segment takes about a third of a line) |
| `spectrumCookie` | `"tp_spectrum"`      | cookie that keeps the `A` key choice for a year (`1` shown, `0` hidden) |

`attach` returns `{ open(index), close(), audio }`, or `null` if there are no links.

## Controls

- Keys: Space — next track, `P` — previous, `<` `>` or `←` `→` — seek 10 seconds, `↑` `↓` — seek a minute,
  `S` — pause, `+` `-` — volume, `A` — show / hide the spectrum analyzer (the window
  resizes to fit, the choice is kept in a cookie), `Esc` or Backspace — close. Keys are matched by `e.code`, so they
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
| `--tp-spec-low`, `--tp-spec-mid`, `--tp-spec-high` | `#1a1`, `#cc0`, `#d22` | analyzer segments: bottom, middle, top |

How the analyzer works: as many bands as fit the window edge to edge (23 at the default width),
spaced evenly on a log scale from 50 Hz to 16 kHz; a band
shows its average power (not its loudest bin), with a tilt of +4.5 dB per octave around 1 kHz,
because music loses energy towards the highs and without the tilt the right half stays low.
Bars rise at once and fall at a fixed rate in seconds (the same on 60 and 120 Hz screens);
a peak segment hangs above each bar for 0.8 s, then sinks slowly. The scale (-80..-36 dB after
the tilt) was measured on the podcasts of this site: a typical level fills about half a bar,
loud moments reach the yellow and red segments.

The analyzer runs only when every link is on the same site as the page: an element routed
through Web Audio plays audio from another origin (without CORS) as silence. Otherwise the
player works as before, without the bars.

The font must be monospaced and include box-drawing (`╔═║`) and block (`█░`) characters,
otherwise the frames fall apart.

## License

MIT, see [LICENSE](../../LICENSE).
