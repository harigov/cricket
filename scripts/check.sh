#!/usr/bin/env bash
# Formatting, lint and test gate for Willow Cricket.
#
#   scripts/check.sh          # check formatting, lint and run tests
#   scripts/check.sh --fix    # rewrite formatting and apply machine-applicable lint fixes
#
# Requires the rustfmt and clippy components. On a rustup toolchain:
#   rustup component add rustfmt clippy
# On a distro toolchain (no rustup, cargo at /usr/bin) install them from apt:
#   sudo apt-get install -y rustfmt rust-clippy
# Linux builds also need the Bevy system deps (libudev, ALSA, X11/Wayland):
#   sudo apt-get install -y libudev-dev libasound2-dev libx11-dev libxkbcommon-dev \
#     libwayland-dev libdecor-0-dev
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "${1:-}" == "--fix" ]]; then
    echo "==> cargo fmt --all"
    cargo fmt --all
    echo "==> cargo clippy --fix"
    cargo clippy --all-targets --fix --allow-dirty --allow-staged
else
    echo "==> cargo fmt --all --check"
    cargo fmt --all -- --check
    echo "==> cargo clippy -D warnings"
    cargo clippy --all-targets -- -D warnings
fi

echo "==> cargo test"
cargo test
