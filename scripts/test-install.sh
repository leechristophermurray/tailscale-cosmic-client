#!/usr/bin/env bash
# Tests for install.sh and package.sh, run in a throwaway HOME.
#
# The binaries are stand-in shell scripts, so this needs no build and never
# touches the real ~/.local or /usr.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
project="$(cd "$here/.." && pwd)"

passed=0
failed=0
failures=()

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

bins="$work/bins"
mkdir -p "$bins"
for binary in cosmic-tailscale cosmic-applet-tailscale; do
    printf '#!/bin/sh\necho %s\n' "$binary" > "$bins/$binary"
    chmod 0755 "$bins/$binary"
done

# Everything install.sh places, relative to the prefix; caches are checked apart.
expected_payload() {
    cat <<'LIST'
bin/cosmic-applet-tailscale
bin/cosmic-tailscale
share/applications/io.github.leechristophermurray.CosmicAppletTailscale.desktop
share/applications/io.github.leechristophermurray.CosmicTailscale.Taildrop.desktop
share/applications/io.github.leechristophermurray.CosmicTailscale.desktop
share/icons/hicolor/scalable/apps/io.github.leechristophermurray.CosmicTailscale-symbolic.svg
share/icons/hicolor/scalable/status/io.github.leechristophermurray.CosmicAppletTailscale-connected-symbolic.svg
share/icons/hicolor/scalable/status/io.github.leechristophermurray.CosmicAppletTailscale-disconnected-symbolic.svg
share/icons/hicolor/scalable/status/io.github.leechristophermurray.CosmicAppletTailscale-exitnode-symbolic.svg
share/metainfo/io.github.leechristophermurray.CosmicTailscale.metainfo.xml
LIST
}

payload() { (cd "$1" && find . -type f ! -name '*.cache' | sed 's|^\./||' | LC_ALL=C sort); }

# Scenarios run inside `if`, where bash ignores `set -e`, so every assertion in
# them ends with an explicit `|| return 1`.
check() {
    local name="$1"
    shift
    local output
    if output="$("$@" 2>&1)"; then
        passed=$((passed + 1))
        printf '  ok    %s\n' "$name"
    else
        failed=$((failed + 1))
        failures+=("$name")
        printf '  FAIL  %s\n%s\n' "$name" "$(printf '%s\n' "$output" | sed 's/^/        /')"
    fi
}

same_payload() { diff <(expected_payload | LC_ALL=C sort) <(payload "$1"); }
empty_tree() { local left; left="$(find "$1" -type f ! -name '*.cache')"; [[ -z "$left" ]] || { echo "left behind: $left"; return 1; }; }

# ---- per-user install -------------------------------------------------------------

scenario_user_install() {
    local home="$work/user-home"
    mkdir -p "$home" || return 1
    HOME="$home" "$project/scripts/install.sh" --bin-dir "$bins" >/dev/null || return 1
    same_payload "$home/.local" || return 1
    [[ "$(stat -c %a "$home/.local/bin/cosmic-tailscale")" == 755 ]] || { echo "binary is not 0755"; return 1; }
    [[ "$(stat -c %a "$home/.local/share/applications/io.github.leechristophermurray.CosmicTailscale.desktop")" == 644 ]] || { echo "desktop entry is not 0644"; return 1; }
    # A per-user icon cache would go stale when other applications add icons.
    [[ ! -e "$home/.local/share/icons/hicolor/icon-theme.cache" ]] || { echo "created a per-user icon cache"; return 1; }
    HOME="$home" "$project/scripts/install.sh" --uninstall >/dev/null || return 1
    empty_tree "$home/.local" || return 1
}
check "per-user install lands in ~/.local and uninstalls cleanly" scenario_user_install

scenario_missing_binaries() {
    local home="$work/missing-home" output
    mkdir -p "$home" "$work/empty-bins"
    if output="$(HOME="$home" "$project/scripts/install.sh" --bin-dir "$work/empty-bins" 2>&1)"; then
        echo "install succeeded without binaries"
        return 1
    fi
    grep -q "Build it first" <<<"$output" || { echo "unhelpful error: $output"; return 1; }
    [[ ! -e "$home/.local" ]] || { echo "installed data files before failing"; return 1; }
}
check "missing binaries fail before anything is installed" scenario_missing_binaries

scenario_legacy_ids() {
    local home="$work/legacy-home" local_share
    local_share="$home/.local/share"
    # What an install from before the app ID change left behind.
    mkdir -p "$local_share/applications" "$local_share/metainfo" \
        "$local_share/icons/hicolor/scalable/apps" "$local_share/icons/hicolor/scalable/status" || return 1
    touch "$local_share/applications/com.system76.CosmicTailscale.desktop" \
        "$local_share/applications/com.system76.CosmicAppletTailscale.desktop" \
        "$local_share/applications/com.system76.CosmicTailscale.Taildrop.desktop" \
        "$local_share/metainfo/com.system76.CosmicTailscale.metainfo.xml" \
        "$local_share/icons/hicolor/scalable/apps/com.system76.CosmicTailscale-symbolic.svg" \
        "$local_share/icons/hicolor/scalable/status/com.system76.CosmicAppletTailscale-connected-symbolic.svg" || return 1
    # Someone else's file with a similar name must survive.
    touch "$local_share/applications/com.system76.CosmicTerm.desktop" || return 1

    HOME="$home" "$project/scripts/install.sh" --bin-dir "$bins" >/dev/null || return 1
    diff <( (expected_payload; echo share/applications/com.system76.CosmicTerm.desktop) | LC_ALL=C sort) \
        <(payload "$home/.local") || return 1
}
check "installing removes files left under the old System76 IDs" scenario_legacy_ids

# ---- staged install, as a package build does --------------------------------------

scenario_staged() {
    local stage="$work/stage"
    HOME="$work" "$project/scripts/install.sh" --prefix /usr --destdir "$stage" --bin-dir "$bins" >/dev/null || return 1
    same_payload "$stage/usr" || return 1
    # Caches in a staged tree would be shipped and then conflict on the system.
    local caches
    caches="$(find "$stage" -name '*.cache')"
    [[ -z "$caches" ]] || { echo "staged caches: $caches"; return 1; }
    HOME="$work" "$project/scripts/install.sh" --uninstall --prefix /usr --destdir "$stage" >/dev/null || return 1
    [[ -z "$(find "$stage" -type f)" ]] || { echo "uninstall left: $(find "$stage" -type f)"; return 1; }
}
check "staged install writes no caches, and uninstall leaves nothing" scenario_staged

# ---- release tarball --------------------------------------------------------------

scenario_tarball() {
    local copy="$work/project" tarball extracted="$work/extracted" home="$work/tar-home"
    # package.sh writes dist/ beside itself, so run it on a copy of the project.
    mkdir -p "$copy" || return 1
    cp -r "$project/Cargo.toml" "$project/README.md" "$project/LICENSE" "$project/data" "$project/scripts" "$copy/" || return 1
    tarball="$(SOURCE_DATE_EPOCH=0 "$copy/scripts/package.sh" "$bins")" || return 1
    (cd "$copy/dist" && sha256sum --quiet -c ./*.sha256) || { echo "checksum mismatch"; return 1; }

    local first
    first="$(sha256sum < "$copy/$tarball")" || return 1
    # A second build, later and from freshly touched sources, must match byte
    # for byte: timestamps from the clock or the checkout must not leak in.
    sleep 1
    find "$copy/data" "$copy/scripts" -exec touch {} + || return 1
    SOURCE_DATE_EPOCH=0 "$copy/scripts/package.sh" "$bins" >/dev/null || return 1
    [[ "$first" == "$(sha256sum < "$copy/$tarball")" ]] || { echo "tarball is not reproducible"; return 1; }

    mkdir -p "$extracted" "$home" || return 1
    tar -xzf "$copy/$tarball" -C "$extracted" || return 1
    (cd "$extracted"/cosmic-tailscale-*/ && HOME="$home" ./install.sh >/dev/null) || return 1
    same_payload "$home/.local" || return 1
    cmp -s "$bins/cosmic-tailscale" "$home/.local/bin/cosmic-tailscale" || { echo "installed the wrong binary"; return 1; }
}
check "release tarball is reproducible and its install.sh works" scenario_tarball

echo
echo "$passed passed, $failed failed"
((failed == 0)) || { printf '  %s\n' "${failures[@]}"; exit 1; }
