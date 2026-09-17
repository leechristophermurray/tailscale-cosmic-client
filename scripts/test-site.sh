#!/usr/bin/env bash
# Tests for build-site.sh.
#
# The page is published into the same directory as the apt repository, so the
# checks here are about what the build writes, what it leaves alone, and that
# nothing it emits points at a file that is not there.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
project="$(cd "$here/.." && pwd)"

passed=0
failed=0
failures=()

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

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

version="$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$project/Cargo.toml")"

# ---- what it writes ---------------------------------------------------------------

scenario_files() {
    local out="$work/site"
    "$project/scripts/build-site.sh" "$out" >/dev/null || return 1
    for file in index.html style.css icon.svg assets/applet-and-window.png \
        assets/machines.png assets/monitoring.png assets/taildrop.png; do
        [[ -s "$out/$file" ]] || { echo "missing $file"; return 1; }
    done
    grep -q "v$version" "$out/index.html" || { echo "the version is not on the page"; return 1; }
}
check "the page and its assets are written" scenario_files

# Every local link and image must resolve, or the page ships a 404.
scenario_links() {
    local out="$work/site" target
    "$project/scripts/build-site.sh" "$out" >/dev/null || return 1
    while read -r target; do
        # Skip absolute URLs, anchors and the apt page, which apt-repo.sh writes.
        [[ "$target" =~ ^(https?:|#|mailto:) || "$target" == "apt/" ]] && continue
        target="${target%%#*}"
        [[ -e "$out/$target" ]] || { echo "index.html points at missing $target"; return 1; }
    done < <(grep -oE '(href|src)="[^"]+"' "$out/index.html" | sed 's/^[a-z]*="//; s/"$//')
}
check "every local link and image resolves" scenario_links

scenario_no_placeholders() {
    local out="$work/site"
    "$project/scripts/build-site.sh" "$out" >/dev/null || return 1
    ! grep -o '@[A-Z_]\+@' "$out/index.html"
}
check "no template placeholder survives the build" scenario_no_placeholders

scenario_env_overrides() {
    local out="$work/env-site"
    SITE_URL="https://example.test/pages" COPR_PROJECT="someone/cosmic-tailscale/" \
        FEDORA_STATUS="Available" \
        "$project/scripts/build-site.sh" "$out" >/dev/null || return 1
    grep -q "https://example.test/pages/cosmic-tailscale.sources" "$out/index.html" ||
        { echo "the site URL was not used in the apt commands"; return 1; }
    # copr-cli reads a trailing slash as an empty project name; so would a reader.
    grep -q "dnf copr enable someone/cosmic-tailscale$" "$out/index.html" ||
        { echo "the COPR project is wrong"; return 1; }
    grep -q '>Available<' "$out/index.html" || { echo "the channel status was not used"; return 1; }
}
check "the site URL, COPR project and channel status can be set" scenario_env_overrides

# HTML5 has no validator to hand — xmllint rejects <header>, <main> and inline
# SVG outright — so this checks the one thing that actually breaks a layout.
scenario_tags_balance() {
    local out="$work/site"
    "$project/scripts/build-site.sh" "$out" >/dev/null || return 1
    python3 - "$out/index.html" <<'PY'
import sys
from html.parser import HTMLParser

VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link",
        "meta", "param", "source", "track", "wbr"}
# The page relies on the parser closing these itself, as browsers do.
OPTIONAL_END = {"html", "head", "body", "p", "li"}


class Balance(HTMLParser):
    def __init__(self):
        super().__init__()
        self.stack = []
        self.problems = []

    def handle_starttag(self, tag, attrs):
        if tag not in VOID:
            self.stack.append((tag, self.getpos()[0]))

    def handle_startendtag(self, tag, attrs):
        # <circle ... /> in the inline SVG: opens and closes at once, so the
        # default of reporting a start and an end would unbalance the stack.
        pass

    def handle_endtag(self, tag):
        if tag in VOID:
            return
        while self.stack:
            open_tag, line = self.stack.pop()
            if open_tag == tag:
                return
            if open_tag not in OPTIONAL_END:
                self.problems.append(f"line {line}: <{open_tag}> closed by </{tag}>")
                return


parser = Balance()
parser.feed(open(sys.argv[1], encoding="utf-8").read())
parser.problems += [
    f"line {line}: <{tag}> is never closed"
    for tag, line in parser.stack
    if tag not in OPTIONAL_END
]
if parser.problems:
    print("\n".join(parser.problems))
    sys.exit(1)
PY
}
check "the page's tags are balanced" scenario_tags_balance

scenario_refuses_to_build_into_its_own_sources() {
    local output
    if output="$(cd "$project" && ./scripts/build-site.sh site 2>&1)"; then
        echo "it built into site/, where the sources live"
        return 1
    fi
    grep -q "sources live" <<<"$output" || { echo "unhelpful error: $output"; return 1; }
    # The sources must be untouched.
    [[ ! -e "$project/site/index.html" ]] || { echo "an index.html was left in site/"; return 1; }
}
check "building into the sources is refused" scenario_refuses_to_build_into_its_own_sources

# ---- what it leaves alone ---------------------------------------------------------

scenario_keeps_apt_repo() {
    local out="$work/shared"
    mkdir -p "$out/dists/stable/main/binary-amd64" "$out/pool/main/c/cosmic-tailscale" "$out/apt" || return 1
    echo "release" > "$out/dists/stable/InRelease"
    echo "deb" > "$out/pool/main/c/cosmic-tailscale/cosmic-tailscale_0.1.0-1_amd64.deb"
    echo "apt page" > "$out/apt/index.html"
    echo "key" > "$out/cosmic-tailscale-archive-keyring.gpg"

    "$project/scripts/build-site.sh" "$out" >/dev/null || return 1

    [[ "$(cat "$out/dists/stable/InRelease")" == "release" ]] || { echo "the apt index was touched"; return 1; }
    [[ "$(cat "$out/apt/index.html")" == "apt page" ]] || { echo "the apt page was overwritten"; return 1; }
    [[ -s "$out/pool/main/c/cosmic-tailscale/cosmic-tailscale_0.1.0-1_amd64.deb" ]] || { echo "a package went missing"; return 1; }
    [[ -s "$out/cosmic-tailscale-archive-keyring.gpg" ]] || { echo "the keyring went missing"; return 1; }
    [[ -s "$out/index.html" ]] || { echo "the page was not written"; return 1; }
}
check "an apt repository in the same directory survives" scenario_keeps_apt_repo

echo
echo "$passed passed, $failed failed"
((failed == 0)) || { printf '  %s\n' "${failures[@]}"; exit 1; }
