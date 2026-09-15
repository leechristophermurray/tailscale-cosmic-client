# Rust's release profile already strips debug info, so there is nothing for a
# -debuginfo subpackage to hold, and rpmbuild would fail trying to make one.
%global debug_package %{nil}

Name:           cosmic-tailscale
Version:        0.1.0
Release:        1%{?dist}
Summary:        Tailscale client and panel applet for the COSMIC desktop

License:        MPL-2.0
URL:            https://github.com/leechristophermurray/tailscale-cosmic-client
Source0:        %{url}/archive/v%{version}/tailscale-cosmic-client-%{version}.tar.gz

ExclusiveArch:  x86_64

BuildRequires:  cargo >= 1.93
BuildRequires:  rust >= 1.93
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  cmake
BuildRequires:  make
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  desktop-file-utils
BuildRequires:  appstream

# Loaded with dlopen at runtime, so rpm cannot find it from the binaries.
Requires:       libwayland-client
Recommends:     tailscale
Recommends:     cosmic-term
# Mounting a machine's files: gio from glib2, GVfs's SFTP backend, and ssh.
Recommends:     cosmic-files
Recommends:     gvfs
Recommends:     openssh-clients

%description
A native Tailscale client for COSMIC. It talks to tailscaled over its LocalAPI
socket, so it reflects the real state of your tailnet without wrapping the
command line.

The panel applet toggles the tunnel, switches exit nodes and copies peer
addresses. The window manages machines, exit nodes, Taildrop, published
services, a Caddy reverse proxy and Beszel hardware monitoring on tailnet peers.

%prep
%autosetup -n tailscale-cosmic-client-%{version}

%build
# Dependencies, libcosmic among them, come from crates.io and git, so the COPR
# project must allow network access during builds.
export CARGO_HOME="%{_builddir}/cargo-home"
export RUSTFLAGS="%{?build_rustflags}"
cargo build --release --locked

%install
./scripts/install.sh --prefix %{_prefix} --destdir %{buildroot} --bin-dir target/release

%check
desktop-file-validate %{buildroot}%{_datadir}/applications/*.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/io.github.leechristophermurray.CosmicTailscale.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/cosmic-tailscale
%{_bindir}/cosmic-applet-tailscale
%{_datadir}/applications/io.github.leechristophermurray.CosmicTailscale.desktop
%{_datadir}/applications/io.github.leechristophermurray.CosmicTailscale.Taildrop.desktop
%{_datadir}/applications/io.github.leechristophermurray.CosmicAppletTailscale.desktop
%{_metainfodir}/io.github.leechristophermurray.CosmicTailscale.metainfo.xml
%{_datadir}/icons/hicolor/scalable/apps/io.github.leechristophermurray.CosmicTailscale-symbolic.svg
%{_datadir}/icons/hicolor/scalable/status/io.github.leechristophermurray.CosmicAppletTailscale-*-symbolic.svg

%changelog
* Tue Sep 15 2026 Christopher Murray <lee.christopher.murray@gmail.com> - 0.1.0-1
- First package
