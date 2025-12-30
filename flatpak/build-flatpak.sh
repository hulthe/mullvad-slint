#!/bin/bash

set -ex

cd "$(dirname 0)"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-../target}"

# TODO: cross compilation
cargo b -r

cp "$CARGO_TARGET_DIR/release/mullvad-slint" .

# TODO: cross compilation with --arch=aarch64
flatpak-builder -v --force-clean --user --install-deps-from=flathub --repo=repo --install builddir net.mullvad.MullvadSlint.yml
