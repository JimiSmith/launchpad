#!/bin/sh
# Installs Launchpad into this checkout's bin/ for the herdr plugin: the
# release archive for this plugin version where one exists and runs here,
# otherwise a build with Cargo.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' herdr-plugin.toml | head -n 1)
[ -n "$version" ] || { echo "No version in herdr-plugin.toml" >&2; exit 1; }
release="https://github.com/JimiSmith/launchpad/releases/download/v$version"

case "$(uname -s)-$(uname -m)" in
    Linux-x86_64 | Linux-amd64) archive=launchpad-linux-x86_64.tar.gz ;;
    Darwin-x86_64) archive=launchpad-macos-x86_64.tar.gz ;;
    Darwin-arm64 | Darwin-aarch64) archive=launchpad-macos-aarch64.tar.gz ;;
    *) archive= ;;
esac
tmp=
target=
trap 'rm -rf "$tmp" "$target"' EXIT

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 2 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        echo "Neither curl nor wget is available" >&2
        return 1
    fi
}
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    else
        shasum -a 256 "$1"
    fi | cut -d ' ' -f 1
}

# A download or checksum failure stops the install; only a missing or
# unusable binary for this platform falls back to Cargo. `set -e` does not
# apply here, because the function runs as a condition.
download() {
    tmp=$(mktemp -d) || exit 1
    echo "Downloading $archive for v$version"
    for file in "$archive" "$archive.sha256"; do
        fetch "$release/$file" "$tmp/$file" || {
            echo "Download failed: $release/$file" >&2
            exit 1
        }
    done
    expected=$(cut -d ' ' -f 1 "$tmp/$archive.sha256")
    actual=$(sha256 "$tmp/$archive")
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        echo "Checksum mismatch for $archive (expected $expected, got $actual)" >&2
        exit 1
    fi
    tar -xzf "$tmp/$archive" -C "$tmp" launchpad || exit 1
    if ! "$tmp/launchpad" --version >/dev/null 2>&1; then
        echo "The release binary does not run here (it needs glibc from Ubuntu 24.04 or newer)"
        return 1
    fi
    { mkdir -p bin && mv -f "$tmp/launchpad" bin/launchpad; } || exit 1
}

build() {
    if ! command -v cargo >/dev/null 2>&1; then
        echo "No usable release archive for $(uname -s) $(uname -m), and cargo is not installed." >&2
        echo "Install Rust from https://rustup.rs, then reinstall the plugin." >&2
        exit 1
    fi
    echo "Building Launchpad with Cargo; this takes a few minutes"
    # Build outside the checkout, which herdr keeps for as long as the plugin.
    target=$(mktemp -d)
    CARGO_TARGET_DIR="$target" cargo build --release --locked -p launchpad
    mkdir -p bin
    cp -f "$target/release/launchpad" bin/launchpad
}

if [ -n "$archive" ] && download; then
    :
else
    build
fi
chmod 755 bin/launchpad
echo "Installed $(bin/launchpad --version)"
