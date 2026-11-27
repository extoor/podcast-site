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
| `/ru/…`, `/ua/…` | the same pages in Russian and Ukrainian (English is the main version; a switcher in the corner of every page) | — |
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

## Container images

Production runs in Kubernetes (the dev server only develops). `docker/Dockerfile` builds
three images, each for `linux/amd64` and `linux/arm64` under one tag:

| Target | What it is | Runs as |
|--------|------------|---------|
| `generator` | `sitegen --scan`: scan the library and build the site in one command | CronJob |
| `gopherd` | the Gopher server | Deployment |
| `web` | Caddy with `caddy/Caddyfile`, plain HTTP on 8080 (TLS at the Ingress) | Deployment |

```bash
docker buildx build --pull --platform linux/amd64,linux/arm64 -f docker/Dockerfile \
  --target gopherd -t <registry>/podcast-site/gopherd:<tag> --push .
```

All images are rootless: they run as UID/GID 65532 (numeric, so Kubernetes' `runAsNonRoot`
can verify it), need no capabilities and no writable root filesystem. The `web` image takes
the newest Caddy 2.x at build time (`caddy:2-alpine` with `--pull`); rebuild it regularly.

The Rust binaries are cross-compiled on the build machine's own architecture into static
musl executables (linked by `rust-lld` from the Rust toolchain; every dependency is pure
Rust), so there is no emulation and no C toolchain. Our images are
`distroless/static:nonroot`: no shell, a non-root user, a few megabytes. The generator is
configured by `PODCAST_BASE_URL`, `GOPHER_HOST`, `GOPHER_PORT`; `gopherd --check <addr>`
is its health check.

## Kubernetes

The Helm chart `deploy/helm/podcast-site` deploys everything:

- the domain is written once, `domain` (plus optional `extraDomains`); the cert-manager
  `Certificate`, both Traefik `IngressRoute`s (`web`, port 80, redirects to HTTPS
  through the `redirect-scheme` middleware; `websecure` has none) and the playlists' base URL all come from it;
- `generator` is a CronJob (`generator.schedule`) plus a Job after every install/upgrade;
  the built site lives on a PVC (`site.storageClass`, `openebs-hostpath` by default, so the
  three pods share one node);
- the audio is a read-only PV of the rclone CSI driver
  ([veloxpack/csi-driver-rclone](https://github.com/veloxpack/csi-driver-rclone),
  `crypt:music/Podcasts`); its configuration (key `configData`, the whole `rclone.conf`) is
  a Secret made outside the chart (`media.csi.nodePublishSecretRef`), or use your own
  claim with `media.type: existingClaim`;
- `gopherd` gets its own address through Multus (`gopher.podAnnotations`) and listens on
  port 70 without root (the pod sysctl `net.ipv4.ip_unprivileged_port_start=0`); menus
  point to `gopher.host` (default: `domain`, the same name as the web);
- every pod passes Pod Security "restricted"; `networkPolicy.enabled` forbids egress and lets
  only Traefik reach the web pod.

```sh
helm install podcast-site deploy/helm/podcast-site -n podcasts --create-namespace \
  --set domain=podcasts.example.org
```

Also there: `deploy/kustomize/` — the same chart through kustomize
(`kustomize build --enable-helm`) for your own patches, and `deploy/argocd/application.yaml`.

## Continuous integration

GitHub Actions (`.github/workflows/`):

- `ci.yml` on every push and pull request: `cargo fmt`, `clippy -D warnings`, tests,
  `cargo audit`, `cargo deny`, the inlined JavaScript modules, the `Caddyfile` with the
  newest Caddy, gitleaks over the whole history (no secrets in this public repository), and
  the Helm chart (`helm lint`, kubeconform with the cert-manager and Traefik schemas).
- `images.yml`: the three images for amd64 + arm64, pushed to
  `ghcr.io/<owner>/<repo>/{generator,gopherd,web}` from `main` and `v*` tags (pull requests
  only build them); a weekly run rebuilds them with the newest Caddy and base images.
- Dependabot keeps the actions, crates and base images up to date.

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
in development, multi-arch rootless images in GHCR, the Helm chart. Next: the first deploy
to the cluster.

## License

[MIT](LICENSE).
