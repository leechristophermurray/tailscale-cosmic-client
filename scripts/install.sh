#!/usr/bin/env bash
# Install or uninstall cosmic-tailscale.
#
# One script for every way this gets installed — `just install`,
# `just install-user`, and the release tarball — so the three cannot drift.
# It works from a source checkout (binaries in target/release, assets in data/)
# and from an extracted release (binaries in bin/, assets beside this script).
#
# Usage:
#   install.sh                       install into ~/.local (no root needed)
#   sudo install.sh --prefix /usr    install system-wide
#   install.sh --uninstall           remove what install put in place
#
# Options:
#   --prefix DIR    installation prefix           (default: ~/.local)
#   --destdir DIR   stage under DIR, for packaging (default: none)
#   --bin-dir DIR   where the built binaries are  (default: detected)
#   --uninstall     remove instead of install

set -euo pipefail

readonly APP_ID="com.system76.CosmicTailscale"
readonly APPLET_ID="com.system76.CosmicAppletTailscale"
readonly BINARIES=(cosmic-tailscale cosmic-applet-tailscale)

here="$(cd "$(dirname "$0")" && pwd)"

prefix="${HOME}/.local"
destdir=""
bin_dir=""
action="install"

while (($# > 0)); do
    case "$1" in
        --prefix) prefix="$2"; shift 2 ;;
        --destdir) destdir="$2"; shift 2 ;;
        --bin-dir) bin_dir="$2"; shift 2 ;;
        --uninstall) action="uninstall"; shift ;;
        -h | --help) sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "unknown option: $1" >&2; exit 2 ;;
    esac
done

# An extracted release keeps data/ next to this script; a checkout keeps it one
# level up, beside scripts/.
if [[ -d "$here/data" ]]; then
    data="$here/data"
elif [[ -d "$here/../data" ]]; then
    data="$(cd "$here/../data" && pwd)"
else
    echo "cannot find the data/ directory next to $0" >&2
    exit 1
fi

if [[ -z "$bin_dir" ]]; then
    if [[ -d "$here/bin" ]]; then
        bin_dir="$here/bin"
    else
        bin_dir="${CARGO_TARGET_DIR:-$here/../target}/release"
    fi
fi

root="${destdir}${prefix}"
applications="$root/share/applications"
metainfo="$root/share/metainfo"
icons="$root/share/icons/hicolor"

# ---- uninstall -------------------------------------------------------------------

if [[ "$action" == "uninstall" ]]; then
    for binary in "${BINARIES[@]}"; do
        rm -f "$root/bin/$binary"
    done
    rm -f "$applications/$APP_ID.desktop" \
          "$applications/$APPLET_ID.desktop" \
          "$applications/$APP_ID.Taildrop.desktop" \
          "$metainfo/$APP_ID.metainfo.xml" \
          "$icons/scalable/apps/$APP_ID-symbolic.svg" \
          "$icons/scalable/status/$APPLET_ID"-*.svg
    if [[ -z "$destdir" ]]; then
        update-desktop-database "$applications" 2>/dev/null || true
    fi
    echo "Removed cosmic-tailscale from $prefix."
    exit 0
fi

# ---- install ----------------------------------------------------------------------

for binary in "${BINARIES[@]}"; do
    if [[ ! -x "$bin_dir/$binary" ]]; then
        echo "$bin_dir/$binary is missing. Build it first: cargo build --release" >&2
        exit 1
    fi
done

for binary in "${BINARIES[@]}"; do
    install -Dm0755 "$bin_dir/$binary" "$root/bin/$binary"
done

install -Dm0644 "$data/applications/$APP_ID.desktop" "$applications/$APP_ID.desktop"
install -Dm0644 "$data/applications/$APPLET_ID.desktop" "$applications/$APPLET_ID.desktop"
install -Dm0644 "$data/applications/$APP_ID.Taildrop.desktop" "$applications/$APP_ID.Taildrop.desktop"
install -Dm0644 "$data/metainfo/$APP_ID.metainfo.xml" "$metainfo/$APP_ID.metainfo.xml"
install -Dm0644 "$data/icons/scalable/apps/$APP_ID-symbolic.svg" "$icons/scalable/apps/$APP_ID-symbolic.svg"
install -Dm0644 -t "$icons/scalable/status" "$data/icons/scalable/status/"*.svg

# A staged tree is finished by the package manager's own triggers, so caches are
# only refreshed when installing for real.
if [[ -z "$destdir" ]]; then
    # Registers the "Send via Taildrop…" handler, which otherwise stays
    # invisible until something else happens to refresh the database.
    update-desktop-database "$applications" 2>/dev/null || true

    # A theme with an icon cache ignores icons missing from it. System trees
    # have one; a per-user ~/.local tree normally does not, and creating one
    # there would go stale as soon as another application added an icon.
    if [[ -f "$icons/icon-theme.cache" || "$prefix" != "$HOME"/* ]]; then
        gtk-update-icon-cache -qtf "$icons" 2>/dev/null || true
    fi
fi

echo "Installed cosmic-tailscale into $prefix."
if [[ -z "$destdir" ]]; then
    echo "Add the panel applet in Settings ▸ Desktop ▸ Panel ▸ Configure panel applets."
fi
