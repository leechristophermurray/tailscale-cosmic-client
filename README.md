# cosmic-tailscale

A native Tailscale client for the [COSMIC](https://system76.com/cosmic) desktop,
built with `libcosmic`. It talks to the local `tailscaled` daemon directly over
its UNIX socket, so it reflects the real state of your tailnet without shelling
out to the `tailscale` binary or parsing its human-readable output.

Two front-ends share one backend:

- **`cosmic-applet-tailscale`** — a panel applet for the things you do in
  passing: toggle the tunnel, switch exit node, copy a peer's address.
- **`cosmic-tailscale`** — a window for everything else: machines, exit nodes,
  published services, access control, and a Caddy manager for tailnet peers.

Both inherit the desktop's theme, accent colour, typography, and density with no
configuration of their own.

## Repository layout

```
crates/
  tailscale-localapi/        the daemon client — no GUI dependencies
    src/
      transport.rs           HTTP/1.1 over the tailscaled UNIX socket
      client.rs              typed calls and the IPN event stream
      model/                 status, prefs, serve, taildrop, ping, ipn
  caddy-admin/               Caddy's JSON admin API, reached across a tailnet
    src/
      client.rs              config reads and structural route edits
      discovery.rs           probe a peer for a reachable admin API
      tunnel.rs              port-forward over Tailscale SSH
  beszel-client/             a Beszel monitoring hub's PocketBase API
    src/
      client.rs              auth, systems, stats, containers
      deploy.rs              agent installation over Tailscale SSH
      model/                 system, stats, container records
  cosmic-tailscale/          the main window
    src/
      app/                   state, messages, async tasks, subscriptions
      pages/                 one module per sidebar destination
      ui/                    formatting, icon names, shared widgets
  cosmic-applet-tailscale/   the panel applet
    src/applet/              popup, panel icon states, activation tokens
data/                        desktop entries, icons, AppStream metadata
```

The dependency direction is strictly one way: `tailscale-localapi` and
`caddy-admin` know nothing about `libcosmic`, which keeps them testable without
a compositor and usable from anything else.

## Requirements

- Tailscale — `just tailscale` installs and configures it if needed
- Rust 1.93 or newer
- `libxkbcommon`, `wayland`, `libinput`, `fontconfig` development packages

## Build and install

On a new machine, one command sets up Tailscale and installs the app:

```sh
just setup
```

`just setup` runs `just tailscale` and then `just install-user`. The Tailscale
step is safe to repeat — on a machine that is already set up it changes nothing
and never asks for `sudo`. It:

1. installs Tailscale if it is missing, from Tailscale's own package
   repositories using their official installer (downloaded to a file and run
   from there, never piped into a shell);
2. enables and starts `tailscaled`;
3. makes you the daemon's **operator**.

The third step is the one that is easy to miss. `tailscaled` accepts changes only
from root or its operator. Without it the app still shows your tailnet, but every
switch — connect, exit node, SSH, DNS — is refused. `install` and `install-user`
print a note if any of these is not in place.

It does not run `tailscale up`: signing in opens a browser, and the app's own
Sign in button does that. Preview the privileged steps with
`DRY_RUN=1 just tailscale`.

Tailscale is deliberately not bundled. `tailscaled` is a privileged system
daemon that needs its own security updates, which Tailscale's repositories
deliver; a copy vendored into this project would quietly stop receiving them. A
distribution package of this app should declare Tailscale as a dependency
instead.

Individually:

```sh
just                 # release build
just test            # run the test suite
just check           # clippy, warnings denied

just install-user    # build and install into ~/.local — no root

just build-release   # ...or system-wide, in two steps
sudo just install
```

Both recipes install the same set: the two binaries, all three desktop entries
(the window, the applet, and the "Send via Taildrop…" file-manager action), the
AppStream metadata, and the icons. Both — along with `uninstall`,
`uninstall-user` and the release tarball — go through one script,
`scripts/install.sh`, so they cannot drift apart. For a distribution package,
stage into a directory with `just rootdir=/path/to/stage install`; a staged tree
gets no `mimeinfo.cache` or icon cache, since the package manager's triggers
build those on the target system.

The system-wide `install` deliberately does not build. Running `sudo just install`
on a recipe that compiled first would run `cargo` as root and leave root-owned
artifacts in `target/`, breaking every later build as your own user.

Installing puts the applet on the system, but does not place it on your panel —
add it in **Settings ▸ Desktop ▸ Panel ▸ Configure panel applets**.

To iterate on the applet, `just dev-install` symlinks the binaries into
`~/.local/bin`; after that `just dev-reload` rebuilds and restarts the panel so
it picks up the new binary.

To run the applet outside the panel, use `just run-applet` rather than invoking
the binary directly. `cosmic-panel` sets `X_PRIVILEGED_WAYLAND_SOCKET` and hands
the matching file descriptor to the applets it launches; a terminal in a COSMIC
session inherits the variable but not the descriptor, and libcosmic's
activation-token thread panics trying to adopt the stale fd. The recipe clears
the variable so the applet connects to Wayland normally.

### From a release

Tagged releases on GitHub carry a tarball built by CI, with a `.sha256` beside
it. It needs no Rust toolchain:

```sh
sha256sum -c cosmic-tailscale-*-x86_64-linux.tar.gz.sha256
tar -xzf cosmic-tailscale-*-x86_64-linux.tar.gz
cd cosmic-tailscale-*-x86_64-linux
./install-tailscale.sh   # if Tailscale is not set up yet
./install.sh             # into ~/.local; or: sudo ./install.sh --prefix /usr
```

`just package` builds the same tarball locally, into `dist/`.

### Poking at it

To see what the daemon reports without starting a GUI:

```sh
just status
```

To reproduce the Caddy connection flow against a peer, without the GUI:

```sh
cargo run -p caddy-admin --example connect -- homeforge
```

## What it does

### Machines

A filterable list of every node, split into this machine, your devices, and
machines shared with you. Each row shows how the peer is actually reached —
a direct WireGuard path or a DERP relay — rather than just "online".

The detail pane has copyable IPv4, IPv6, and MagicDNS addresses, a latency probe
that reports the real path, and one-click **SSH terminal**, which opens
`tailscale ssh` in `cosmic-term`. Authentication comes from your tailnet
identity, so there are no keys to distribute.

### Exit nodes

Pick a peer to carry your internet traffic, or advertise this machine as an exit
node. When a tailnet admin has not yet approved an advertised route, the page
says so — advertising alone does not make a machine usable.

### Services & DNS

MagicDNS state and what this machine publishes with `tailscale serve` (tailnet
only) or `tailscale funnel` (the public internet). Funnelled services are marked
distinctly, because the difference is who can reach them.

### Caddy

Manage a Caddy reverse proxy running on any tailnet peer. Caddy is configured
through its JSON admin API, so routes are edited structurally and applied live —
no Caddyfile to parse and no service restart.

The admin API normally binds to the peer's own localhost. The page tries the
peer's tailnet address first, and falls back to forwarding the port over
Tailscale SSH, so the admin port is never exposed to the network. A hostname
ending in `.ts.net` gets its HTTPS certificate from Tailscale automatically.

Two details are worth knowing, because both produce confusing failures:

- Caddy validates the `Host` header of every admin request against its
  configured origins. Through a tunnel the port we dial is one Caddy has never
  heard of, so requests claim the loopback origin it allows by default — and are
  sent in origin-form, because Go's HTTP server takes `r.Host` from the URL
  authority of an absolute-form request line and ignores the header entirely.
- A Caddyfile compiles to nested `subroute` handlers, so the raw route list is
  one entry containing every upstream on the machine. The page walks the tree to
  the leaves to recover the `host -> upstream` pairs that were actually
  configured. Only routes this client created carry an `@id`, so only those can
  be removed; the rest are labelled as coming from the Caddyfile.

### Monitoring

Hardware health from a [Beszel](https://beszel.dev) hub on your tailnet, beside
the machines it belongs to: CPU, memory, disk, load, sensors, ZFS pools and
per-container stats. The hub never needs to be on the public internet.

Beszel's records use very short keys — memory percent is `mp`, temperatures are
`t` — because they are written per machine per interval and kept for months. Two
things are worth knowing about how they are read:

- **`mu` is already the committed figure.** The agent subtracts buffers, cache
  and the ZFS ARC from it and reports each separately. Treating them as
  components of `mu` and subtracting again makes a machine using 4.7 GiB report
  zero. `memory_reclaimable()` reports them alongside, never inside.
- **Load average is shown against the thread count.** `0.8` is idle on sixteen
  threads and saturated on one. Where the thread count is unknown, no figure is
  given rather than a misleading one.

Each machine gets time-series charts — CPU, memory, disk, disk I/O, bandwidth,
load and temperature — over a selectable window, with one period control above
all of them rather than one per card. They are drawn natively on an `iced`
canvas, so they inherit the desktop theme like everything else.

Three things about how they are coloured, since none of it is arbitrary:

- **Single-series charts use the desktop accent.** One mark means no pairs to
  keep apart, and an accent is guaranteed legible on its own surface.
- **Multi-series charts do not.** A user's accent is arbitrary — this machine's
  light accent is a dark desaturated teal, below both the lightness band and the
  chroma floor a categorical slot must clear. Fine alone, unfit beside two
  siblings. Those charts use a fixed blue/orange/aqua palette instead, validated
  all-pairs against COSMIC's real surfaces in both modes (worst CVD ΔE 9.2 light
  / 9.4 dark against a target of 8). A series therefore keeps its colour when the
  theme changes.
- **Temperature uses emphasis, not eight colours.** A machine reporting eight
  sensors would need eight hues that no one could tell apart; instead the hottest
  is drawn in the accent and the rest recede to context, with the peak named in
  the caption.

Axes keep a zero baseline but scale their ceiling to the data. Truncating the
baseline is what makes a molehill look like a mountain; refusing to scale the
ceiling is what draws a machine sitting at 18% as a flat line six pixels tall.

Machines the hub is not monitoring are listed separately, and can have the agent
installed over Tailscale SSH. That runs a vendor script as root, so the exact
command is shown, on the named host, and nothing runs until you approve that
specific one — there is deliberately no way to approve a batch.

Credentials: the hub address and account go in `cosmic-config`; the password
goes to the system keyring via the freedesktop secret service. The session token
is reused between refreshes, so a background poll sends no password.

### Access controls

Node key expiry with a countdown, Tailscale SSH, shields-up, and the subnet
routes this machine advertises. When a tailnet has key expiry disabled, the page
says that rather than inventing a countdown.

## Desktop integration

- **Taildrop** — its own page, both directions: choose a machine, then choose
  files (or drop them), with received files and a Save action below. The drop
  zone opens the desktop's own file chooser through the XDG portal, and the
  **Send file** button on a machine does the same and starts the transfer as
  soon as you pick. This is the dependable path.
- **Dragging** — the window is also a drop target, accepting both `text/uri-list`
  and the portal's file-transfer handover. This is best-effort: winit's
  `FileDropped` event never fires on Wayland, so the drop goes through
  libcosmic's `dnd_destination` instead, and whether it arrives depends on what
  the sending application offers.
- **File manager** — "Send via Taildrop…" registers as a handler for a broad
  list of file types, so it appears under *Open With*, and hands the selection
  to the window to pick a machine. There is no cross-desktop MIME type meaning
  "any file" (`all/all` is a KDE convention that `update-desktop-database`
  discards, registering nothing), so the list in the desktop entry is
  explicit.
- **Notifications** — an arriving Taildrop transfer offers **Save to
  Downloads**, which fetches the file from the daemon and only then clears it
  from the queue, so a failed write cannot lose the transfer. A node key within
  a week of expiry warns with a **Reauthenticate** action.
- **Terminal** — **SSH terminal** opens a `cosmic-term` tab running
  `tailscale ssh`.
- **Panel** — the applet icon has a distinct silhouette per state (off,
  connecting, connected, exit node) rather than a distinct colour, because the
  panel renders symbolic icons in a single tint.

## How it stays in sync

`tailscaled` pushes state changes over its IPN bus, so the UI is event-driven
rather than poll-driven: flipping the switch in `tailscale up` updates the window
immediately. The bus does not carry the peer map, so a slow timer re-reads that
alongside it — and it is what notices the daemon came back after a restart. The
bus subscription reconnects on its own.

Every write goes through the daemon and the UI reconciles against what comes
back, rather than assuming the write succeeded.

## Localization

Both binaries use Fluent catalogues, embedded at build time:

```
crates/cosmic-tailscale/i18n/en/cosmic-tailscale.ftl
crates/cosmic-applet-tailscale/i18n/en/cosmic-applet-tailscale.ftl
```

Add a language by copying the `en` directory to its language code and
translating the values. Nothing needs registering — the catalogue directory is
embedded, and the desktop's language preference selects from it at startup.

## Testing

```sh
just test        # Rust tests, installer scenarios, packaging checks
just coverage    # line coverage of product code (--html for a report)
```

`just test` runs four layers:

- **Rust tests** — decoding against a redacted capture of real daemon output;
  the update loop driven message by message; every page rendered headlessly;
  and each service client talking to `http-stub`, a stand-in server that records
  requests exactly as sent. That last layer is what catches wire-level bugs — an
  absolute-form request line, a notify mask one bit off — that no function-level
  mock can see.
- **`scripts/test-install-tailscale.sh`** — the installer run against simulated
  machines, with fake `tailscale`, `systemctl`, `curl` and `sudo`. It installs
  nothing and downloads nothing, so it behaves the same on any machine.
- **`scripts/test-install.sh`** — `install.sh` and `package.sh` in a throwaway
  `HOME` with stand-in binaries: the per-user, staged and tarball installs place
  exactly the expected files, uninstall removes them, and two builds of the
  tarball are byte-identical.
- **`scripts/check-packaging.sh`** — shellcheck, `desktop-file-validate` and
  `appstreamcli`, each reported as skipped rather than passed when the tool is
  not installed (`STRICT=1` fails instead, as CI does).

`just coverage` needs `cargo-llvm-cov` and LLVM tools matching rustc's LLVM
(`rustup component add llvm-tools-preview`, or a system `llvm-cov` of the same
major version, which it finds itself). It counts product code only: tests live
in trailing `#[cfg(test)]` modules, which always execute and would otherwise
report themselves as covered, so the script drops them, along with `tests/`,
`examples/` and the test-support crate.

Tests that spawn a fake `ssh` hold a shared lock while writing the script and
spawning it. Without it, a parallel test forking mid-write inherits the open
file, and the kernel refuses to execute it (`ETXTBSY`) — which failed these
tests about a third of the time.

Still untested: `view` code outside the pages, chart drawing (canvas `draw`
only runs with a renderer), subscriptions, the keyring, and notifications.
Wayland drag-and-drop and the applet inside a real `cosmic-panel` can only be
checked by hand.

## Continuous integration

`.github/workflows/ci.yml` runs on every push to `main` and every pull request:

| Job | What it runs |
| --- | --- |
| Format | `cargo fmt --check` |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings` |
| Test | `cargo test --workspace` |
| Scripts and packaging | both script test suites, then `check-packaging.sh` with `STRICT=1`, in a Fedora container (see below) |
| Minimum Rust version | `cargo check` on the `rust-version` from `Cargo.toml` |
| Coverage | `scripts/coverage.sh`; the table goes to the run summary, `lcov.info` to an artifact |
| Package | release build and tarball, uploaded as an artifact (not on pull requests) |

Every Cargo step uses `--locked`, so a `Cargo.lock` that is out of date fails
rather than being silently rewritten. The runner needs only `libxkbcommon-dev`
(the one library the binaries link) and `cmake` (for `aws-lc-sys`) beyond the
usual build tools — a list found by building the workspace in a bare
`ubuntu:24.04` container rather than copied from another project. The shared
setup lives in `.github/actions/setup`.

The scripts job runs in a `fedora:44` container for one reason: COSMIC's
desktop entries use `Categories=COSMIC`, which desktop-file-utils registered in
0.28, and Ubuntu 24.04 still ships 0.27. The container runs as root, and the
installer suite, whose scenarios are all a desktop user running the installer,
re-runs itself as `nobody` there.

Third-party actions are pinned to full commit SHAs, with the version in a
comment; Dependabot proposes updates to them, and to crates, weekly. `libcosmic`
is excluded — it is pinned to a git revision, and moving it is a change to test
deliberately. The Rust cache is written only from `main`, so pull requests read
it but cannot alter what `main` builds from.

### Releasing

1. Bump `version` under `[workspace.package]` in `Cargo.toml`, and commit.
2. Tag it and push the tag:

   ```sh
   git tag -s v0.2.0 -m v0.2.0
   git push origin v0.2.0
   ```

`.github/workflows/release.yml` then checks that the tag matches the version in
`Cargo.toml`, runs the whole of CI against the tagged commit, verifies the
tarball's checksum, and publishes a GitHub release with it and generated notes.
A tag with a suffix, such as `v0.2.0-rc.1`, becomes a pre-release. The publish
job is the only one with write access to the repository, and it builds nothing:
it ships the tarball that CI built and tested in the same run.

## License

MPL-2.0
