#!/usr/bin/env bash
# Static checks on everything that is not Rust: shell scripts, desktop entries
# and AppStream metadata.
#
# Each check runs when its tool is installed and is reported as skipped when it
# is not, so this never fails just because a machine lacks a linter — but it
# never quietly passes a check it did not run, either. CI sets STRICT=1, which
# turns a skipped check into a failure.

set -euo pipefail
cd "$(dirname "$0")/.."

failed=0
ran=0
skipped=()

section() { printf '\n%s\n' "$1"; }

run_check() {
    local tool="$1"
    shift
    if ! command -v "$tool" >/dev/null 2>&1; then
        skipped+=("$tool")
        printf '  skipped: %s is not installed\n' "$tool"
        return
    fi
    ran=$((ran + 1))
    if "$@"; then
        printf '  ok\n'
    else
        failed=$((failed + 1))
    fi
}

section "shellcheck: scripts/*.sh"
run_check shellcheck shellcheck -x scripts/*.sh

section "desktop-file-validate: data/applications/*.desktop"
check_desktop() {
    local status=0
    for entry in data/applications/*.desktop; do
        # Hints are advice; errors are not.
        if ! desktop-file-validate "$entry" >/dev/null 2>&1; then
            desktop-file-validate "$entry"
            status=1
        fi
        # `all/all` and other made-up types validate as a hard error only in
        # newer versions, and register nothing today. Catch them regardless.
        if grep -qE '^MimeType=.*\ball/' "$entry"; then
            printf '  %s: all/* is not a MIME type and registers nothing\n' "$entry"
            status=1
        fi
    done
    return "$status"
}
run_check desktop-file-validate check_desktop

section "appstreamcli: data/metainfo/*.xml"
check_metainfo() {
    local report
    report="$(appstreamcli validate --no-net --no-color data/metainfo/*.xml 2>&1 || true)"
    # The missing homepage is deliberate until the project has a public home;
    # warnings are shown, errors fail.
    printf '%s\n' "$report" | grep -E '^(E|W):' | sed 's/^/  /' || true
    ! printf '%s\n' "$report" | grep -qE '^E:'
}
run_check appstreamcli check_metainfo

echo
if ((failed > 0)); then
    echo "$failed packaging check(s) failed"
    exit 1
fi
if ((ran == 0)); then
    echo "no packaging checks ran: install shellcheck, desktop-file-utils or appstream"
    exit 1
fi
if [[ "${STRICT:-0}" == 1 ]] && ((${#skipped[@]} > 0)); then
    echo "STRICT=1 and these checks did not run: ${skipped[*]}"
    exit 1
fi
printf '%d packaging check(s) passed' "$ran"
((${#skipped[@]} > 0)) && printf ', skipped: %s' "${skipped[*]}"
echo
