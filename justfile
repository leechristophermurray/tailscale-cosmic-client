# cosmic-tailscale — build, install, and iterate
#
# Panel applets are loaded by cosmic-panel, so changing one means reinstalling
# it and restarting the panel. `just dev-install` symlinks the binaries so that
# loop is one command (`just dev-reload`) rather than a full reinstall.

app-id := 'com.system76.CosmicTailscale'
applet-id := 'com.system76.CosmicAppletTailscale'

app-bin := 'cosmic-tailscale'
applet-bin := 'cosmic-applet-tailscale'

rootdir := ''
prefix := '/usr'

base-dir := absolute_path(clean(rootdir / prefix))
cargo-target-dir := env('CARGO_TARGET_DIR', 'target')
release-dir := cargo-target-dir / 'release'

# System-wide destinations
bin-dir := base-dir / 'bin'
desktop-dir := base-dir / 'share' / 'applications'
metainfo-dir := base-dir / 'share' / 'metainfo'
icon-dir := base-dir / 'share' / 'icons' / 'hicolor' / 'scalable'

# User-local destinations (no root, works on immutable distributions)
user-base := env('HOME') / '.local'
user-bin-dir := user-base / 'bin'
user-desktop-dir := user-base / 'share' / 'applications'
user-metainfo-dir := user-base / 'share' / 'metainfo'
user-icon-dir := user-base / 'share' / 'icons' / 'hicolor' / 'scalable'

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

# Run the test suite
test *args:
    cargo test --workspace {{args}}

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
    install -Dm0755 {{release-dir / app-bin}} {{bin-dir / app-bin}}
    install -Dm0755 {{release-dir / applet-bin}} {{bin-dir / applet-bin}}
    install -Dm0644 data/applications/{{app-id}}.desktop {{desktop-dir / app-id}}.desktop
    install -Dm0644 data/applications/{{applet-id}}.desktop {{desktop-dir / applet-id}}.desktop
    install -Dm0644 data/applications/{{app-id}}.Taildrop.desktop {{desktop-dir / app-id}}.Taildrop.desktop
    install -Dm0644 data/metainfo/{{app-id}}.metainfo.xml {{metainfo-dir / app-id}}.metainfo.xml
    install -Dm0644 data/icons/scalable/apps/{{app-id}}-symbolic.svg {{icon-dir}}/apps/{{app-id}}-symbolic.svg
    install -Dm0644 -t {{icon-dir}}/status data/icons/scalable/status/*.svg
    @update-desktop-database {{desktop-dir}} 2>/dev/null || true
    @gtk-update-icon-cache -qtf {{base-dir}}/share/icons/hicolor 2>/dev/null || true

# Install into ~/.local — no root required
install-user: build-release _check-tailscale
    install -Dm0755 {{release-dir / app-bin}} {{user-bin-dir / app-bin}}
    install -Dm0755 {{release-dir / applet-bin}} {{user-bin-dir / applet-bin}}
    install -Dm0644 data/applications/{{app-id}}.desktop {{user-desktop-dir / app-id}}.desktop
    install -Dm0644 data/applications/{{applet-id}}.desktop {{user-desktop-dir / applet-id}}.desktop
    install -Dm0644 data/applications/{{app-id}}.Taildrop.desktop {{user-desktop-dir / app-id}}.Taildrop.desktop
    install -Dm0644 data/metainfo/{{app-id}}.metainfo.xml {{user-metainfo-dir / app-id}}.metainfo.xml
    install -Dm0644 data/icons/scalable/apps/{{app-id}}-symbolic.svg {{user-icon-dir}}/apps/{{app-id}}-symbolic.svg
    install -Dm0644 -t {{user-icon-dir}}/status data/icons/scalable/status/*.svg
    @update-desktop-database {{user-desktop-dir}} 2>/dev/null || true
    @echo "Installed. Add the applet in Settings ▸ Desktop ▸ Panel ▸ Configure panel applets."

# Symlink the release binaries into ~/.local so rebuilds take effect in place
dev-install: build-release
    mkdir -p {{user-bin-dir}}
    ln -sf {{absolute_path(release-dir / app-bin)}} {{user-bin-dir / app-bin}}
    ln -sf {{absolute_path(release-dir / applet-bin)}} {{user-bin-dir / applet-bin}}
    install -Dm0644 data/applications/{{app-id}}.desktop {{user-desktop-dir / app-id}}.desktop
    install -Dm0644 data/applications/{{applet-id}}.desktop {{user-desktop-dir / applet-id}}.desktop
    install -Dm0644 data/applications/{{app-id}}.Taildrop.desktop {{user-desktop-dir / app-id}}.Taildrop.desktop
    install -Dm0644 data/metainfo/{{app-id}}.metainfo.xml {{user-metainfo-dir / app-id}}.metainfo.xml
    install -Dm0644 data/icons/scalable/apps/{{app-id}}-symbolic.svg {{user-icon-dir}}/apps/{{app-id}}-symbolic.svg
    install -Dm0644 -t {{user-icon-dir}}/status data/icons/scalable/status/*.svg
    @update-desktop-database {{user-desktop-dir}} 2>/dev/null || true
    @echo "Add the applet in Settings ▸ Desktop ▸ Panel, then run 'just dev-reload' after edits."

# Rebuild and restart the panel so it picks up the new applet binary
dev-reload: build-release
    pkill cosmic-panel || true

uninstall:
    rm -f {{bin-dir / app-bin}} {{bin-dir / applet-bin}}
    rm -f {{desktop-dir / app-id}}.desktop {{desktop-dir / applet-id}}.desktop {{desktop-dir / app-id}}.Taildrop.desktop
    rm -f {{metainfo-dir / app-id}}.metainfo.xml
    rm -f {{icon-dir}}/apps/{{app-id}}-symbolic.svg
    rm -f {{icon-dir}}/status/{{applet-id}}-*.svg

uninstall-user:
    rm -f {{user-bin-dir / app-bin}} {{user-bin-dir / applet-bin}}
    rm -f {{user-desktop-dir / app-id}}.desktop {{user-desktop-dir / applet-id}}.desktop {{user-desktop-dir / app-id}}.Taildrop.desktop
    rm -f {{user-metainfo-dir / app-id}}.metainfo.xml
    rm -f {{user-icon-dir}}/apps/{{app-id}}-symbolic.svg
    rm -f {{user-icon-dir}}/status/{{applet-id}}-*.svg
