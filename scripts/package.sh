#!/usr/bin/env bash
# Build a release tarball from already-compiled binaries.
#
# The tarball holds the binaries, the data files and install.sh, laid out so
# that `./install.sh` works straight after extracting. Alongside it goes a
# SHA256SUMS file.
#
# Usage: scripts/package.sh [RELEASE_DIR]   (default: target/release)

set -euo pipefail
cd "$(dirname "$0")/.."

release_dir="${1:-${CARGO_TARGET_DIR:-target}/release}"
version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
arch="$(uname -m)"
name="cosmic-tailscale-${version}-${arch}-linux"
stage="dist/$name"

if [[ -z "$version" ]]; then
    echo "could not read the workspace version from Cargo.toml" >&2
    exit 1
fi

rm -rf "$stage" "dist/$name.tar.gz"
mkdir -p "$stage/bin"

for binary in cosmic-tailscale cosmic-applet-tailscale; do
    if [[ ! -x "$release_dir/$binary" ]]; then
        echo "$release_dir/$binary is missing. Build it first: cargo build --release" >&2
        exit 1
    fi
    install -m0755 "$release_dir/$binary" "$stage/bin/$binary"
done

cp -r data "$stage/data"
install -m0755 scripts/install.sh "$stage/install.sh"
install -m0755 scripts/install-tailscale.sh "$stage/install-tailscale.sh"
cp README.md LICENSE "$stage/"

# Reproducible ordering and ownership, so identical inputs give identical bytes.
tar --sort=name --owner=0 --group=0 --numeric-owner \
    --mtime="@${SOURCE_DATE_EPOCH:-0}" \
    -C dist -czf "dist/$name.tar.gz" "$name"
rm -rf "$stage"

(cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256")
echo "dist/$name.tar.gz"
