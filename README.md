# podcast-site

**English** | [Русский](README.ru.md)

A small podcast catalog served over two protocols at once, **Gopher** and **HTTP(S)**.
The catalog is built from a directory of MP3 files: one scan, then static menus,
pages and M3U8 playlists that the servers only hand out.

- **Web:** retro pages drawn with text characters only (a pixel banner made of `█`,
  box-drawing frames, no images), a DOS-style audio player in the page, full keyboard
  control.
- **Gopher:** the same catalog as menus with the same banner; episodes and playlists
  (`.m3u`) are downloaded over Gopher. Menu text uses only characters of CP866, so it
  shows correctly in Gopher browsers on the ZX Spectrum.
- **Playlists:** M3U8 for every podcast, every folder and all episodes, for VLC,
  foobar2000, mpv and the like.

## How it works

```
/media/podcasts ──podscan──► build/catalog.json ──sitegen──► build/gopher/**/gophermap ──► gopherd (Gopher)
   (MP3 files)        └────► build/playlists/*.m3u8      └──► build/www/**/index.html  ──► Caddy (HTTP/S)
                                                                                     audio and playlists ─┘
```

One set of data, two outputs. The generators run on demand or on a schedule,
never per request; the servers only read files.

| Part | What it does |
|------|--------------|
| [`podscan`](podscan/) | Scans the media library. Reads the ID3 tag and the first MPEG frame of each file (version, bitrate, CBR/VBR, sample rate, channels, duration) in one small read, without downloading the file. Without tags, artist and title are parsed from the file name. Writes `catalog.json` and the playlists. |
| [`podcasts.toml`](podcasts.toml) | Rules that assign episodes to podcasts (shows): spellings of the album tag and file name prefixes. |
| [`sitegen`](sitegen/) | Builds the Gopher menus and the HTML pages from `catalog.json`. |
| [`gopherd`](gopherd/) | Read-only Gopher server (Rust, tokio). |
| [`caddy/Caddyfile`](caddy/Caddyfile) | Web server config: pages, playlists, audio with Range requests (seeking). |
| [`web/textplayer`](web/textplayer/) | The DOS-style audio player; a standalone module with no dependencies, usable in other projects. |
| [`web/keynav`](web/keynav/) | Keyboard navigation for the pages: ↑↓ to select, Enter to open or play, Backspace to go back. |

Terms: a **podcast** is a show (e.g. Zebra), a **folder** is a directory in the media
library (e.g. KissFM). A folder can hold episodes of many podcasts.

## Addresses

The web and Gopher use the same paths:

| Path | Web | Gopher |
|------|-----|--------|
| `/` | index | root menu |
| `/podcast/<slug>/` | episodes of a podcast | menu |
| `/dir/<slug>/` | episodes of a folder | menu |
| `/playlists/…` | M3U8, downloaded as a file | M3U8 (type 9) |
| `/media/<path>` | MP3 (only `*.mp3`) | MP3 (type 9) |

## Running it

Development happens in the dev container (`.devcontainer/`, Rust image); it installs
Caddy and publishes ports 7070 (Gopher) and 8080 (HTTP) on the host. The media library
is mounted at `/media/podcasts`, read-only.

```bash
# 1. scan the library; the base URL goes into the playlists
cargo run --release -p podscan -- --base-url http://<host>:8080

# 2. build menus and pages; the Gopher host and port go into every menu line
cargo run --release -p sitegen -- --gopher-host <host> --gopher-port 7070

# 3. servers
cargo run --release -p gopherd -- --listen 0.0.0.0:7070
caddy run --config caddy/Caddyfile --adapter caddyfile
```

Then open `http://<host>:8080/` in a browser and `gopher://<host>:7070/` in a Gopher
client (or `curl gopher://<host>:7070/1/`). Each tool prints its options with `--help`.

For production, Caddy takes `SITE_ADDRESS` (the domain; it gets a certificate itself),
`BUILD_DIR` and `MEDIA_DIR` from the environment.

## Security

- `gopherd` validates every selector before touching the disk (no `..`, hidden
  files, absolute paths, NUL or control bytes), resolves every path including symlinks
  and serves it only if it stays inside its root. Request length, read and write
  timeouts and the number of connections are limited. It only reads files.
  Attacks are covered by integration tests (`gopherd/tests/server.rs`).
- Caddy serves only MP3 files from the media library, sends a Content Security Policy
  that allows only the pages' own inline code and Google Fonts, and has its admin API
  turned off.
- Everything taken from tags and file names is escaped in HTML and cleaned of tabs
  and line breaks in Gopher menus.
- No `unsafe` code; `cargo audit` and `cargo deny` run before every commit.

## Status

Working: scanning, playlists, Gopher menus and server, HTML pages, the player, Caddy
in development. Next: Docker images (multi-stage, non-root, read-only file system),
then Kubernetes. Still open: the site's domain.

## License

MIT (as declared in the `Cargo.toml` files).
