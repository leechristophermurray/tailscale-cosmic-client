#!/usr/bin/env bash
# Build the project's web page.
#
# The page is published to GitHub Pages beside the apt repository, which lives
# at the same site root (dists/, pool/ and the keyring), so this only ever
# writes the files it owns and never clears the directory it is given.
#
# Usage: scripts/build-site.sh [OUT_DIR]
#   OUT_DIR   where to write (default: dist/site)
#
# SITE_URL sets the absolute base used in the apt commands and social metadata;
# it defaults to the project's Pages address.

set -euo pipefail
cd "$(dirname "$0")/.."

out="${1:-dist/site}"
mkdir -p "$out/assets"

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
repo="${REPO_URL:-https://github.com/leechristophermurray/tailscale-cosmic-client}"
site="${SITE_URL:-https://leechristophermurray.github.io/tailscale-cosmic-client}"
copr="${COPR_PROJECT:-iamthinkking/cosmic-tailscale}"
copr="${copr%/}"

# Channels that are not published yet say so, rather than offering a command
# that cannot work.
fedora_status="${FEDORA_STATUS:-Coming soon}"
aur_status="${AUR_STATUS:-Coming soon}"

substitute() {
    sed -e "s|@VERSION@|$version|g" \
        -e "s|@REPO@|$repo|g" \
        -e "s|@SITE@|${site%/}|g" \
        -e "s|@COPR@|$copr|g" \
        -e "s|@FEDORA_STATUS@|$fedora_status|g" \
        -e "s|@AUR_STATUS@|$aur_status|g" \
        "$1"
}

substitute site/index.html.in > "$out/index.html"
cp site/style.css site/icon.svg "$out/"
cp docs/assets/img/screenshots/*.png "$out/assets/"

if grep -o '@[A-Z_]\+@' "$out/index.html" | sort -u | grep .; then
    echo "the page still has placeholders" >&2
    exit 1
fi

echo "$out"
