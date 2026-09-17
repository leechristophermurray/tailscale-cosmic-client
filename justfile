# cosmic-tailscale — build, install, and iterate
#
# Panel applets are loaded by cosmic-panel, so changing one means reinstalling
# it and restarting the panel. `just dev-install` symlinks the binaries so that
# loop is one command (`just dev-reload`) rather than a full reinstall.

app-bin := 'cosmic-tailscale'
applet-bin := 'cosmic-applet-tailscale'

rootdir := ''
prefix := '/usr'

cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
release-dir := cargo-target-dir / 'release'

# Per-user installs go here: no root, and works on immutable distributions
user-base := env('HOME') / '.local'

default: build-release

# Install Tailscale if missing, start it, and let you manage it without sudo
tailscale:
    ./scripts/install-tailscale.sh

# Everything a new machine needs: Tailscale, then this app, into ~/.local
setup: tailscale install-user

# Warn — without failing — when the app would install but could not work.
_check-tailscale:
    #!/usr/bin/env bash
    if ! command -v tailscaled >/dev/null 2>&1; then
        echo "note: Tailscale is not installed. Run 'just tailscale' (or 'just setup') first."
    elif ! systemctl is-active --quiet tailscaled 2>/dev/null; then
        echo "note: tailscaled is not running. Run 'just tailscale' to enable it."
    elif [[ "$(tailscale debug prefs 2>/dev/null | sed -n 's/.*"OperatorUser": *"\([^"]*\)".*/\1/p')" != "$(id -un)" ]]; then
        echo "note: $(id -un) is not the Tailscale operator, so the app will be read-only."
        echo "      Run 'just tailscale' to fix that."
    fi

# Compile with the debug profile
build-debug *args:
    cargo build {{args}}

# Compile with the release profile
build-release *args: (build-debug '--release' args)

# Lint everything, denying warnings so CI fails on them
check *args:
    cargo clippy --workspace --all-targets {{args}} -- -D warnings

# Run every test: Rust, the install scripts, and packaging checks
test *args:
    cargo test --workspace {{args}}
    ./scripts/test-install-tailscale.sh
    ./scripts/test-install.sh
    ./scripts/test-site.sh
    ./scripts/check-packaging.sh

# Build the project page into dist/site (SITE_URL sets the published address)
site out='dist/site':
    ./scripts/build-site.sh {{out}}
    @echo "Preview it with: python3 -m http.server -d {{out}}"

# Line coverage of product code, excluding test modules (--html for a report)
coverage *args:
    ./scripts/coverage.sh {{args}}

# Run the main window against the local tailscaled
run *args:
    env RUST_LOG=cosmic_tailscale=debug,warn cargo run --bin {{app-bin}} {{args}}

# cosmic-panel sets X_PRIVILEGED_WAYLAND_SOCKET and passes the matching file
# descriptor to the applets it launches. A terminal inside a COSMIC session
# inherits the variable but not the descriptor, so libcosmic's activation-token
# thread tries to adopt a stale fd and panics with "Bad file descriptor".
# Clearing it makes the applet connect the ordinary way.
#
# Run the applet outside the panel, for quick checks
run-applet *args:
    env -u X_PRIVILEGED_WAYLAND_SOCKET RUST_LOG=cosmic_applet_tailscale=debug,warn \
        cargo run --bin {{applet-bin}} {{args}}

# Print what the local daemon reports, without starting a GUI
status:
    cargo run -p tailscale-localapi --example smoke

clean:
    cargo clean

# Fail with something actionable rather than a missing-file error from install
_require-build:
    @test -x {{release-dir / app-bin}} && test -x {{release-dir / applet-bin}} || { \
        echo "Binaries missing. Run 'just build-release' as your user first."; exit 1; }

# This recipe deliberately does not build: if it did, `sudo just install` would
# compile as root and leave root-owned artifacts in target/.
#
# Install system-wide (build first, then run under sudo)
install: _require-build _check-tailscale
    ./scripts/install.sh --prefix {{prefix}} {{ if rootdir != '' { '--destdir ' + absolute_path(rootdir) } else { '' } }} --bin-dir {{release-dir}}

# Install into ~/.local — no root required
install-user: build-release _check-tailscale
    ./scripts/install.sh --prefix {{user-base}} --bin-dir {{release-dir}}

# Build a release tarball in dist/: binaries, data files and the install script
package: build-release
    ./scripts/package.sh {{release-dir}}

# Symlink the release binaries into ~/.local so rebuilds take effect in place
dev-install: build-release
    ./scripts/install.sh --prefix {{user-base}} --bin-dir {{release-dir}}
    ln -sf {{absolute_path(release-dir / app-bin)}} {{user-base / 'bin' / app-bin}}
    ln -sf {{absolute_path(release-dir / applet-bin)}} {{user-base / 'bin' / applet-bin}}
    @echo "Run 'just dev-reload' after edits."

# Rebuild and restart the panel so it picks up the new applet binary
dev-reload: build-release
    pkill cosmic-panel || true

uninstall:
    ./scripts/install.sh --uninstall --prefix {{prefix}} {{ if rootdir != '' { '--destdir ' + absolute_path(rootdir) } else { '' } }}

uninstall-user:
    ./scripts/install.sh --uninstall --prefix {{user-base}}
