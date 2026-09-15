#!/usr/bin/env bash
# Tests for install-tailscale.sh, run against simulated machines.
#
# Every tool the installer touches — tailscale, tailscaled, systemctl, curl and
# sudo — is replaced by a fake that reads and writes a small state directory.
# Nothing is installed, no service is changed and nothing is downloaded, so this
# runs the same on a machine with Tailscale, without it, or in CI.

set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"

# Every scenario is a desktop user running the installer. Root takes other paths
# (it never needs sudo), so under root — as in a CI container — the whole suite
# re-runs as an unprivileged account instead of reporting those paths as bugs.
if [[ $EUID -eq 0 && -z "${TEST_INSTALL_UNPRIVILEGED:-}" ]]; then
    exec setpriv --reuid="$(id -u nobody)" --regid="$(id -g nobody)" --clear-groups \
        env TEST_INSTALL_UNPRIVILEGED=1 HOME=/tmp bash "$here/$(basename "$0")" "$@"
fi

installer="$here/install-tailscale.sh"
me="$(id -un)"

passed=0
failed=0
failures=()

# ---- simulated machine ---------------------------------------------------------

# A fresh sandbox: real core utilities, fake everything Tailscale-related.
new_machine() {
    ln="$(command -v ln)"
    sandbox="$(mktemp -d)"
    bin="$sandbox/bin"
    state="$sandbox/state"
    mkdir -p "$bin" "$state"
    : > "$state/sudo.log"
    : > "$state/curl.log"

    # Only these real tools are reachable, so a real tailscale on this machine
    # can never leak into a test.
    for tool in bash sh mktemp sed id sleep head rm cat chmod printf dirname touch; do
        if path="$(command -v "$tool" 2>/dev/null)" && [[ "$path" == /* ]]; then
            ln -s "$path" "$bin/$tool"
        fi
    done

    cat > "$bin/sudo" <<SUDO
#!/bin/sh
printf '%s\n' "\$*" >> "$state/sudo.log"
exec "\$@"
SUDO

    cat > "$bin/curl" <<CURL
#!/bin/sh
printf '%s\n' "\$*" >> "$state/curl.log"
out=""
while [ \$# -gt 0 ]; do
    [ "\$1" = "--output" ] && out="\$2"
    shift
done
if [ -f "$state/empty-download" ]; then : > "\$out"; exit 0; fi
# The "official installer": installing puts tailscale on the PATH immediately,
# as a real package install does, so later steps in the same run can use it.
cat > "\$out" <<INSTALL
#!/bin/sh
touch "$state/installed"
$ln -sf "$sandbox/tailscale" "$bin/tailscale"
$ln -sf "$sandbox/tailscaled" "$bin/tailscaled"
INSTALL
CURL

    cat > "$bin/systemctl" <<SYSTEMCTL
#!/bin/sh
case "\$1" in
    is-active)  [ -f "$state/active" ] ;;
    is-enabled) [ -f "$state/enabled" ] ;;
    enable)     touch "$state/active" "$state/enabled" ;;
    *)          exit 1 ;;
esac
SYSTEMCTL

    # tailscale and tailscaled exist only once "installed".
    cat > "$sandbox/tailscale" <<TAILSCALE
#!/bin/sh
case "\$1 \$2" in
    "version "*) echo "1.100.0" ;;
    "debug prefs") printf '{\n\t"OperatorUser": "%s",\n}\n' "\$(cat "$state/operator" 2>/dev/null)" ;;
    "status --json") [ -f "$state/active" ] ;;
    "status "*) [ -f "$state/loggedin" ] ;;
    "set "*)
        for arg in "\$@"; do
            case "\$arg" in --operator=*) printf '%s' "\${arg#--operator=}" > "$state/operator" ;; esac
        done ;;
    *) exit 1 ;;
esac
TAILSCALE
    printf '#!/bin/sh\nexit 0\n' > "$sandbox/tailscaled"

    chmod +x "$bin"/sudo "$bin"/curl "$bin"/systemctl "$sandbox/tailscale" "$sandbox/tailscaled"
}

# Put tailscale on the simulated machine (or re-link it after the fake install).
link_tailscale() {
    ln -sf "$sandbox/tailscale" "$bin/tailscale"
    ln -sf "$sandbox/tailscaled" "$bin/tailscaled"
}

configured_machine() {
    new_machine
    link_tailscale
    touch "$state/active" "$state/enabled" "$state/loggedin"
    printf '%s' "$me" > "$state/operator"
}

# Run the installer against the simulated machine.
run() {
    set +e
    output="$(env -i HOME="$HOME" PATH="$bin" "$@" bash "$installer" 2>&1)"
    status=$?
    set -e
}

sudo_calls() { wc -l < "$state/sudo.log" | tr -d ' '; }

# ---- assertions ---------------------------------------------------------------

check() {
    local description="$1"
    shift
    if "$@"; then
        passed=$((passed + 1))
    else
        failed=$((failed + 1))
        failures+=("$current: $description")
    fi
}

scenario() {
    current="$1"
    printf '  %s\n' "$current"
}

finish() {
    rm -rf "$sandbox"
}

# ---- scenarios -----------------------------------------------------------------

echo "install-tailscale.sh"

scenario "an already configured machine is left alone"
configured_machine
run
check "exits 0" test "$status" -eq 0
check "never uses sudo" test "$(sudo_calls)" -eq 0
check "downloads nothing" test ! -s "$state/curl.log"
finish

scenario "a fresh machine is installed, started and given an operator"
new_machine
run
check "exits 0" test "$status" -eq 0
check "fetches the official installer" grep -q "https://tailscale.com/install.sh" "$state/curl.log"
check "runs the installer as root" grep -q "^sh " "$state/sudo.log"
check "enables and starts the service" grep -q "^systemctl enable --now tailscaled" "$state/sudo.log"
check "makes the desktop user the operator" test "$(cat "$state/operator")" = "$me"
check "does not sign in on the user's behalf" test ! -f "$state/loggedin"
finish

scenario "running it again after installing changes nothing"
new_machine
run
: > "$state/sudo.log"
run
check "exits 0" test "$status" -eq 0
check "second run uses no sudo" test "$(sudo_calls)" -eq 0
finish

scenario "a stopped service is started without reinstalling"
configured_machine
rm -f "$state/active"
run
check "exits 0" test "$status" -eq 0
check "starts the service" grep -q "^systemctl enable --now tailscaled" "$state/sudo.log"
check "does not reinstall" test ! -s "$state/curl.log"
finish

scenario "the wrong operator is corrected"
configured_machine
printf 'someone-else' > "$state/operator"
run
check "sets the operator" test "$(cat "$state/operator")" = "$me"
check "only that step uses sudo" test "$(sudo_calls)" -eq 1
finish

scenario "run under sudo, the invoking user becomes operator, not root"
configured_machine
printf 'nobody' > "$state/operator"
run SUDO_USER=alice
check "operator is the sudo user" test "$(cat "$state/operator")" = "alice"
finish

scenario "SUDO_USER=root falls back to the current user"
configured_machine
printf 'nobody' > "$state/operator"
run SUDO_USER=root
check "root is never made operator" test "$(cat "$state/operator")" = "$me"
finish

scenario "OPERATOR overrides the detected user"
configured_machine
printf 'nobody' > "$state/operator"
run OPERATOR=bob SUDO_USER=alice
check "explicit operator wins" test "$(cat "$state/operator")" = "bob"
finish

scenario "an empty download is refused rather than run"
new_machine
touch "$state/empty-download"
run
check "exits non-zero" test "$status" -ne 0
check "never runs the empty installer" test "$(sudo_calls)" -eq 0
finish

scenario "without curl on a fresh machine it stops with a reason"
new_machine
rm -f "$bin/curl"
run
check "exits non-zero" test "$status" -ne 0
check "names what is missing" grep -q "curl" <<< "$output"
finish

scenario "without systemd it stops with a reason"
configured_machine
rm -f "$bin/systemctl"
run
check "exits non-zero" test "$status" -ne 0
check "explains" grep -q "systemd" <<< "$output"
finish

scenario "a dry run on a fresh machine changes nothing"
new_machine
run DRY_RUN=1
check "exits 0" test "$status" -eq 0
check "uses no sudo" test "$(sudo_calls)" -eq 0
check "shows the privileged steps" grep -q "\[dry run\]" <<< "$output"
check "does not claim work was done" grep -q "would be:" <<< "$output"
check "leaves no operator set" test ! -s "$state/operator"
finish

echo
echo "  $passed passed, $failed failed"
if ((failed > 0)); then
    printf '  FAILED: %s\n' "${failures[@]}"
    exit 1
fi
