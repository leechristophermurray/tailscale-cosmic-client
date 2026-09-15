#!/usr/bin/env bash
# Check that every place a version is written agrees with Cargo.toml.
#
# Usage: scripts/check-versions.sh [TAG]
#   TAG, when given (as the release workflow does), must be v<version>.

set -euo pipefail
cd "$(dirname "$0")/.."

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' Cargo.toml)"
failed=0

expect() {
    local what="$1" found="$2"
    if [[ "$found" == "$version" ]]; then
        printf '  ok    %-40s %s\n' "$what" "$found"
    else
        printf '  FAIL  %-40s %s (Cargo.toml says %s)\n' "$what" "${found:-missing}" "$version"
        failed=1
    fi
}

echo "workspace version $version"
if [[ -n "${1:-}" ]]; then
    expect "release tag" "${1#v}"
fi
expect "packaging/rpm/cosmic-tailscale.spec" \
    "$(sed -n 's/^Version:[[:space:]]*//p' packaging/rpm/cosmic-tailscale.spec)"
expect "spec %changelog, newest entry" \
    "$(sed -n '/^%changelog/,$p' packaging/rpm/cosmic-tailscale.spec | sed -n '2s/.* - \([^-]*\)-[0-9]*$/\1/p')"
for pkgbuild in packaging/arch/*/PKGBUILD; do
    expect "$pkgbuild" "$(sed -n 's/^pkgver=//p' "$pkgbuild")"
done
expect "metainfo, newest <release>" \
    "$(grep -o '<release version="[^"]*"' data/metainfo/*.metainfo.xml | head -1 | sed 's/.*version="\(.*\)"/\1/')"

exit "$failed"
