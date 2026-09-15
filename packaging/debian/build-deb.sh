#!/usr/bin/env bash
# Build a .deb from already-compiled binaries.
#
# The file layout comes from scripts/install.sh, so the package installs exactly
# what `just install` and the tests check. Library dependencies are worked out
# by dpkg-shlibdeps from the binaries themselves; Wayland is added by hand,
# because the binaries load it with dlopen and so do not declare it.
#
# Usage: packaging/debian/build-deb.sh BIN_DIR [OUT_DIR]
#   BIN_DIR   directory holding cosmic-tailscale and cosmic-applet-tailscale
#   OUT_DIR   where the .deb goes (default: dist)
#
# Needs dpkg-dev and binutils, plus the libraries the binaries link against
# (libxkbcommon0) installed, since dpkg-shlibdeps reads their symbols files.
#
# DEB_REVISION sets the Debian revision (default: 1). SOURCE_DATE_EPOCH, when
# set, fixes the timestamps so a rebuild is byte-identical.

set -euo pipefail

bin_dir="$(cd "$1" && pwd)"
cd "$(dirname "$0")/../.."
out="${2:-dist}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
revision="${DEB_REVISION:-1}"
arch="$(dpkg --print-architecture)"
name="cosmic-tailscale_${version}-${revision}_${arch}.deb"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
root="$work/root"

./scripts/install.sh --prefix /usr --destdir "$root" --bin-dir "$bin_dir" >/dev/null

# Cargo's release profile drops debug info but keeps the symbol table, which
# Debian policy says a package must not ship.
strip --strip-unneeded --remove-section=.comment --remove-section=.note \
    "$root/usr/bin/cosmic-tailscale" "$root/usr/bin/cosmic-applet-tailscale"

doc="$root/usr/share/doc/cosmic-tailscale"
install -d "$doc"
# MPL-2.0 ships with base-files, so the copyright file points to it.
cat > "$doc/copyright" <<COPYRIGHT
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: cosmic-tailscale
Source: https://github.com/leechristophermurray/tailscale-cosmic-client

Files: *
Copyright: 2026 Christopher Murray <lee.christopher.murray@gmail.com>
License: MPL-2.0
 On Debian systems, the full text of the Mozilla Public License 2.0 can be
 found in /usr/share/common-licenses/MPL-2.0.
COPYRIGHT
changelog_date="$(date -u -R ${SOURCE_DATE_EPOCH:+-d "@$SOURCE_DATE_EPOCH"})"
cat <<CHANGELOG | gzip -9n > "$doc/changelog.Debian.gz"
cosmic-tailscale (${version}-${revision}) stable; urgency=medium

  * Release ${version}. See
    https://github.com/leechristophermurray/tailscale-cosmic-client/releases

 -- Christopher Murray <lee.christopher.murray@gmail.com>  ${changelog_date}
CHANGELOG

# dpkg-shlibdeps insists on a debian/control in the working directory.
mkdir -p "$work/debian"
printf 'Source: cosmic-tailscale\n\nPackage: cosmic-tailscale\nArchitecture: any\n' > "$work/debian/control"
shlibs="$(cd "$work" && dpkg-shlibdeps -O root/usr/bin/cosmic-tailscale root/usr/bin/cosmic-applet-tailscale |
    sed -n 's/^shlibs:Depends=//p')"
if [[ -z "$shlibs" ]]; then
    echo "dpkg-shlibdeps found no library dependencies" >&2
    exit 1
fi

mkdir -p "$root/DEBIAN"
(cd "$root" && find usr -type f -print0 | LC_ALL=C sort -z | xargs -0 md5sum) > "$root/DEBIAN/md5sums"

cat > "$root/DEBIAN/control" <<CONTROL
Package: cosmic-tailscale
Version: ${version}-${revision}
Architecture: ${arch}
Maintainer: Christopher Murray <lee.christopher.murray@gmail.com>
Installed-Size: $(du -sk --apparent-size "$root/usr" | cut -f1)
Depends: ${shlibs}, libwayland-client0
Recommends: tailscale, cosmic-term, cosmic-files, gvfs-backends, libglib2.0-bin, openssh-client
Section: net
Priority: optional
Homepage: https://github.com/leechristophermurray/tailscale-cosmic-client
Description: Tailscale client and panel applet for the COSMIC desktop
 A native Tailscale client for COSMIC. It talks to tailscaled over its
 LocalAPI socket, so it reflects the real state of your tailnet without
 wrapping the command line.
 .
 The panel applet toggles the tunnel, switches exit nodes and copies peer
 addresses. The window manages machines, exit nodes, Taildrop, published
 services, a Caddy reverse proxy and Beszel hardware monitoring on tailnet
 peers.
 .
 Tailscale itself comes from Tailscale's own repositories.
CONTROL

dpkg-deb --root-owner-group -Zxz --build "$root" "$out/$name" >/dev/null
echo "$out/$name"
