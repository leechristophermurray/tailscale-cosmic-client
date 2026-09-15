#!/usr/bin/env bash
# Publish the PKGBUILDs in packaging/arch/ to the AUR for one version.
#
# For each package: set pkgver, reset pkgrel, compute real checksums from the
# published sources, generate .SRCINFO, and push to the AUR if anything changed.
# Runs on Arch (for makepkg and updpkgsums) as an unprivileged user.
#
# Usage: scripts/publish-aur.sh VERSION [--dry-run]
#
# Needs AUR_SSH_KEY_FILE, a private key registered with the AUR account, unless
# --dry-run is given, in which case nothing is pushed and each package's
# PKGBUILD and .SRCINFO are left in $AUR_WORKDIR (default: a temp directory).

set -euo pipefail

version="$1"
dry_run="${2:-}"
here="$(cd "$(dirname "$0")/.." && pwd)"
work="${AUR_WORKDIR:-$(mktemp -d)}"

if [[ "$dry_run" != "--dry-run" ]]; then
    : "${AUR_SSH_KEY_FILE:?set AUR_SSH_KEY_FILE to the AUR deploy key}"
    export GIT_SSH_COMMAND="ssh -i $AUR_SSH_KEY_FILE -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes -o UserKnownHostsFile=$here/packaging/arch/aur_known_hosts"
fi

for dir in "$here"/packaging/arch/*/; do
    pkg="$(basename "$dir")"
    repo="$work/$pkg"
    echo "== $pkg $version"

    if [[ "$dry_run" == "--dry-run" ]]; then
        mkdir -p "$repo"
    else
        rm -rf "$repo"
        git clone -q "ssh://aur@aur.archlinux.org/$pkg.git" "$repo"
    fi

    sed -e "s/^pkgver=.*/pkgver=$version/" -e "s/^pkgrel=.*/pkgrel=1/" "$dir/PKGBUILD" > "$repo/PKGBUILD"
    (cd "$repo" && updpkgsums && makepkg --printsrcinfo > .SRCINFO)
    if grep -q "sha256sums = SKIP" "$repo/.SRCINFO"; then
        echo "$pkg: checksums were not filled in" >&2
        exit 1
    fi

    if [[ "$dry_run" == "--dry-run" ]]; then
        echo "dry run: $repo"
        continue
    fi

    cd "$repo"
    git add PKGBUILD .SRCINFO
    if git diff --cached --quiet; then
        echo "$pkg: already up to date"
    else
        git -c user.name="Christopher Murray" -c user.email="lee.christopher.murray@gmail.com" \
            commit -q -m "Update to $version"
        git push -q origin HEAD:master
        echo "$pkg: published"
    fi
    cd "$here"
done
