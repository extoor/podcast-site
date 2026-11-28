# syntax=docker/dockerfile:1
#
# Images of podcast-site, multi-arch (linux/amd64 + linux/arm64), one target each:
#   generator  sitegen --scan: scans the media library and builds the site (a CronJob)
#   gopherd    the Gopher server
#   web        Caddy with caddy/Caddyfile: pages, playlists, audio
#   caddy      only Caddy with the log plugin (/out/caddy): CI validates the Caddyfile with it
#
# Build and push (from the repository root):
#   docker buildx build --pull --platform linux/amd64,linux/arm64 -f docker/Dockerfile \
#     --target gopherd -t <registry>/podcast-site/gopherd:<tag> --push .
#
# Rootless: every image runs as UID/GID 65532 (numeric, so that Kubernetes'
# runAsNonRoot can check it) and needs no capabilities and no writable root filesystem.
#
# The Rust stage runs on the build machine's own architecture and cross-compiles static
# musl binaries for the target with rust-lld from the Rust toolchain: every dependency is
# pure Rust, so no C toolchain and no emulation are needed.

# Base images are written out in FROM lines (not ARGs) so Dependabot can update them.
# Caddy is the floating caddy:2-alpine / caddy:2-builder-alpine: the newest 2.x at build
# time; CI builds with --pull and rebuilds weekly, so the web image always carries the
# current Caddy.

# --- caddy: Caddy with the transform-encoder log plugin --------------------------------
# The access log is in the Combined Log Format, which only this plugin (from the Caddy
# project) can write. xcaddy builds the builder image's Caddy version; Go cross-compiles,
# so this stage runs on the build machine. The plugin has no releases: pinned to a commit.
FROM --platform=$BUILDPLATFORM docker.io/library/caddy:2-builder-alpine AS caddy
ARG TARGETOS TARGETARCH
ARG TRANSFORM_ENCODER=github.com/caddyserver/transform-encoder@ba4124974830222da7f12a091cf11ddf4d49363f
# XCADDY_SETCAP=0: the builder image would give the binary cap_net_bind_service, and a
# binary with file capabilities can't start once every capability is dropped
RUN GOOS=$TARGETOS GOARCH=$TARGETARCH CGO_ENABLED=0 XCADDY_SETCAP=0 \
    xcaddy build --output /out/caddy --with "$TRANSFORM_ENCODER"

# --- build -----------------------------------------------------------------------------
FROM --platform=$BUILDPLATFORM docker.io/library/rust:1.100.0-slim-bookworm AS build
ARG TARGETARCH
# the commit shown on the site as its version (sitegen/build.rs); there is no .git here
ARG GIT_COMMIT=""
WORKDIR /src
RUN case "$TARGETARCH" in \
      amd64) t=x86_64-unknown-linux-musl ;; \
      arm64) t=aarch64-unknown-linux-musl ;; \
      *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
 && echo "$t" > /rust-target \
 && rustup target add "$t"
COPY Cargo.toml Cargo.lock ./
COPY podscan podscan
COPY sitegen sitegen
COPY gopherd gopherd
# inlined into the pages at compile time (include_str!)
COPY web web
# The crate download cache is shared by the amd64 and arm64 builds running side by side:
# locked, or they unpack the same crates at once ("File exists").
RUN --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/src/target,id=podcast-site-target-${TARGETARCH} \
    t="$(cat /rust-target)" \
 && T="$(echo "$t" | tr 'a-z-' 'A-Z_')" \
 && export "CARGO_TARGET_${T}_LINKER=rust-lld" RUSTFLAGS="-C linker-flavor=ld.lld" \
 && cargo build --release --locked --target "$t" -p sitegen -p gopherd \
 && mkdir -p /out/site \
 && cp "target/$t/release/sitegen" "target/$t/release/gopherd" /out/

# --- generator: CronJob ----------------------------------------------------------------
# Needs: /media (read-only), /site (writable). Settings by environment:
#   PODCAST_BASE_URL  site URL for the playlists, e.g. https://podcasts.example.org
#   GOPHER_HOST, GOPHER_PORT  where the Gopher menus point (the public name and port)
FROM gcr.io/distroless/static-debian12:nonroot AS generator
COPY --from=build /out/sitegen /usr/local/bin/sitegen
# /site owned by the image user: a fresh volume mounted there inherits it (in Kubernetes
# the pod sets fsGroup: 65532 as well)
COPY --from=build --chown=65532:65532 /out/site /site
COPY podcasts.toml /etc/podcast-site/podcasts.toml
USER 65532:65532
ENTRYPOINT ["/usr/local/bin/sitegen", "--scan"]
CMD ["--media", "/media", "--build", "/site", "--rules", "/etc/podcast-site/podcasts.toml"]

# --- gopherd: Deployment ---------------------------------------------------------------
# Needs: /media and /site (both read-only). Listens on 7070 by default; to listen on 70
# without root, give the pod the sysctl net.ipv4.ip_unprivileged_port_start=0.
FROM gcr.io/distroless/static-debian12:nonroot AS gopherd
COPY --from=build /out/gopherd /usr/local/bin/gopherd
COPY --from=build --chown=65532:65532 /out/site /site
USER 65532:65532
EXPOSE 7070
ENTRYPOINT ["/usr/local/bin/gopherd"]
CMD ["--listen", "0.0.0.0:7070", "--build", "/site", "--media", "/media"]
HEALTHCHECK --interval=30s --timeout=5s \
  CMD ["/usr/local/bin/gopherd", "--check", "127.0.0.1:7070"]

# --- web: Deployment -------------------------------------------------------------------
# Caddy on 8080 (plain HTTP: TLS is the Ingress's job), BUILD_DIR=/site, MEDIA_DIR=/media.
# The official binary is replaced with ours (with the log plugin). It has no file
# capabilities, unlike the official one with cap_net_bind_service (see XCADDY_SETCAP
# above): with all capabilities dropped (Pod Security "restricted") such a binary cannot
# even start, and we do not need low ports.
FROM docker.io/library/caddy:2-alpine AS web
COPY --from=caddy /out/caddy /usr/bin/caddy
RUN caddy version \
 && caddy list-modules | grep -qx caddy.logging.encoders.transform
COPY caddy/Caddyfile /etc/caddy/Caddyfile
COPY --from=build --chown=65532:65532 /out/site /site
ENV SITE_ADDRESS=:8080 BUILD_DIR=/site MEDIA_DIR=/media \
    XDG_DATA_HOME=/tmp/caddy-data XDG_CONFIG_HOME=/tmp/caddy-config
USER 65532:65532
EXPOSE 8080
CMD ["caddy", "run", "--config", "/etc/caddy/Caddyfile", "--adapter", "caddyfile"]
