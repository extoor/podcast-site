#!/bin/sh
# Runs once when the dev container is created (postCreateCommand).
set -eu

# Volumes are created by root: hand them to the user.
sudo chown -R vscode:vscode /home/vscode/.claude /usr/local/cargo/registry /home/vscode/.config/gh

# Caddy from its official Debian repository (always the current release; the Debian
# package is far behind), as https://caddyserver.com/docs/install#debian-ubuntu-raspbian
# describes. Chromium: browser checks of the pages (headless).
sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q debian-keyring debian-archive-keyring apt-transport-https curl gnupg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/gpg.key \
  | sudo gpg --batch --yes --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt \
  | sudo tee /etc/apt/sources.list.d/caddy-stable.list >/dev/null
sudo chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
sudo apt-get update -q
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -q caddy chromium
# The access log's Combined format needs the transform-encoder plugin (the web image
# builds Caddy with it, see docker/Dockerfile). An apt upgrade of caddy drops it again:
# run this line once more after one.
sudo caddy add-package github.com/caddyserver/transform-encoder

# Rust: musl targets for the static binaries of the images (amd64 and arm64), wasm32 for
# the player engine (audiocore);
# audit and license checks run before every commit.
rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl wasm32-unknown-unknown
cargo install cargo-audit cargo-deny --locked

# Commit signing: the public key (the secret one stays on the Mac, the agent is forwarded).
gpg --import .devcontainer/signing-key.asc
echo 'E1D456A80E71B7AF640F9B5A9D5A6C199F1B5404:6:' | gpg --import-ownertrust
