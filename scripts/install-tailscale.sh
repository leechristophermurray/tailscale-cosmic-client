#!/usr/bin/env bash
# Make sure tailscaled is installed, running, and operable by the desktop user.
#
# Safe to run repeatedly: every step checks before it acts, so on a machine
# that already has Tailscale set up this changes nothing.
#
# What it does, in order:
#   1. Install Tailscale if it is missing, from Tailscale's own package
#      repositories via their official installer.
#   2. Enable and start the tailscaled service.
#   3. Set the desktop user as the daemon's operator.
#
# Step 3 is the one that is easy to miss. tailscaled only accepts changes from
# root or its configured operator. Without it, the COSMIC client still shows
# your tailnet but every switch — connect, exit node, SSH, DNS — is refused.
#
# It deliberately does NOT run `tailscale up`: signing in opens a browser and
# is interactive, and the client's own Sign in button does exactly that.
#
# Environment:
#   DRY_RUN=1   print the privileged commands instead of running them
#   OPERATOR    user to make operator (default: whoever invoked sudo, or $USER)

set -euo pipefail

readonly INSTALLER_URL="https://tailscale.com/install.sh"

say() { printf '  %s\n' "$*"; }

# Past tense for what happened, conditional for what a dry run only showed.
did() {
    if [[ "${DRY_RUN:-}" == "1" ]]; then
        say "would be: $*"
    else
        say "$*"
    fi
}
step() { printf '\n==> %s\n' "$*"; }

# Run a command as root, or just show it under DRY_RUN.
as_root() {
    if [[ "${DRY_RUN:-}" == "1" ]]; then
        printf '  [dry run] %s\n' "$*"
        return 0
    fi
    if [[ $EUID -eq 0 ]]; then
        "$@"
    else
        sudo "$@"
    fi
}

# The account that should operate the daemon: the person at the desktop, not
# root, even when this script is run under sudo.
operator_user() {
    if [[ -n "${OPERATOR:-}" ]]; then
        printf '%s' "$OPERATOR"
    elif [[ -n "${SUDO_USER:-}" && "$SUDO_USER" != "root" ]]; then
        printf '%s' "$SUDO_USER"
    else
        id -un
    fi
}

# --- 1. install -----------------------------------------------------------------

step "Tailscale"

if command -v tailscaled >/dev/null 2>&1 && command -v tailscale >/dev/null 2>&1; then
    say "already installed: $(tailscale version 2>/dev/null | head -n1)"
else
    say "not installed"

    if ! command -v curl >/dev/null 2>&1; then
        say "curl is required to fetch the installer; install it and re-run."
        exit 1
    fi

    # Tailscale maintains per-distribution repositories and one installer that
    # knows which to add, including the Ubuntu-derived ones COSMIC ships on.
    # It is fetched to a file and run from there rather than piped into a
    # shell, so a truncated download fails instead of running half a script.
    installer="$(mktemp --suffix=-tailscale-install.sh)"
    trap 'rm -f "$installer"' EXIT

    say "fetching the official installer from $INSTALLER_URL"
    curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
        "$INSTALLER_URL" --output "$installer"

    if [[ ! -s "$installer" ]]; then
        say "the installer download was empty; not running it."
        exit 1
    fi

    say "installing (this adds Tailscale's package repository)"
    as_root sh "$installer"
fi

# --- 2. service -----------------------------------------------------------------

step "tailscaled service"

if ! command -v systemctl >/dev/null 2>&1; then
    say "no systemd found; start tailscaled with your init system, then re-run."
    exit 1
fi

# Checking is read-only, so it happens in a dry run too — only the change below
# is skipped. Otherwise a dry run would claim work that is not needed.
if systemctl is-active --quiet tailscaled && systemctl is-enabled --quiet tailscaled; then
    say "enabled and running"
else
    as_root systemctl enable --now tailscaled
    did "enabled and started"
fi

# --- 3. operator ----------------------------------------------------------------

step "Operator"

operator="$(operator_user)"

# The daemon needs a moment after starting before its socket answers.
if command -v tailscale >/dev/null 2>&1; then
    for _ in {1..20}; do
        tailscale status --json >/dev/null 2>&1 && break
        sleep 0.25
    done
fi

current="$(tailscale debug prefs 2>/dev/null | sed -n 's/.*"OperatorUser": *"\([^"]*\)".*/\1/p' || true)"

if [[ "$current" == "$operator" ]]; then
    say "$operator can already manage Tailscale without sudo"
else
    as_root tailscale set --operator="$operator"
    did "$operator can manage Tailscale without sudo"
fi

step "Done"
if tailscale status >/dev/null 2>&1; then
    say "Tailscale is connected."
else
    say "Open the Tailscale app and use Sign in to join your tailnet."
fi
