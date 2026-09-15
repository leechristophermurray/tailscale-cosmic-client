#!/usr/bin/env bash
# Build a signed apt repository from a directory of .deb files.
#
# The result is a static tree ready to serve from any web host (the release
# workflow publishes it to GitHub Pages):
#
#   dists/stable/{Release,InRelease,Release.gpg}
#   dists/stable/main/binary-<arch>/Packages{,.gz}
#   pool/main/c/cosmic-tailscale/*.deb
#   cosmic-tailscale-archive-keyring.gpg   the public key, for Signed-By
#   cosmic-tailscale.sources               a ready-made deb822 sources file
#   index.html                             how to add the repository
#
# Usage: scripts/apt-repo.sh DEB_DIR OUT_DIR BASE_URL
#
# Signing uses the secret key already in the GnuPG keyring ($GNUPGHOME).
# SIGNING_KEY picks one by fingerprint when there are several;
# SIGNING_PASSPHRASE unlocks it when it has a passphrase.

set -euo pipefail

debs="$(cd "$1" && pwd)"
mkdir -p "$2"
out="$(cd "$2" && pwd)"
base_url="${3%/}"

readonly SUITE=stable COMPONENT=main NAME=cosmic-tailscale
keyring_file="$NAME-archive-keyring.gpg"

shopt -s nullglob
packages=("$debs"/*.deb)
if ((${#packages[@]} == 0)); then
    echo "no .deb files in $debs" >&2
    exit 1
fi

gpg_sign=(gpg --batch --yes)
[[ -n "${SIGNING_KEY:-}" ]] && gpg_sign+=(--local-user "$SIGNING_KEY")
if [[ -n "${SIGNING_PASSPHRASE:-}" ]]; then
    gpg_sign+=(--pinentry-mode loopback --passphrase-fd 3)
fi
sign() {
    if [[ -n "${SIGNING_PASSPHRASE:-}" ]]; then
        "${gpg_sign[@]}" "$@" 3<<<"$SIGNING_PASSPHRASE"
    else
        "${gpg_sign[@]}" "$@"
    fi
}

rm -rf "$out/dists" "$out/pool"
pool="pool/$COMPONENT/${NAME:0:1}/$NAME"
mkdir -p "$out/$pool"
cp "${packages[@]}" "$out/$pool/"

cd "$out"
architectures=()
for deb in "$pool"/*.deb; do
    architectures+=("$(dpkg-deb --field "$deb" Architecture)")
done
mapfile -t architectures < <(printf '%s\n' "${architectures[@]}" | LC_ALL=C sort -u)

for arch in "${architectures[@]}"; do
    index="dists/$SUITE/$COMPONENT/binary-$arch"
    mkdir -p "$index"
    apt-ftparchive --arch "$arch" packages "pool" > "$index/Packages"
    gzip -9nkf "$index/Packages"
done

# Written outside dists/ first: apt-ftparchive would otherwise list the
# half-written Release file inside itself.
apt-ftparchive \
    -o APT::FTPArchive::Release::Origin="$NAME" \
    -o APT::FTPArchive::Release::Label="$NAME" \
    -o APT::FTPArchive::Release::Suite="$SUITE" \
    -o APT::FTPArchive::Release::Codename="$SUITE" \
    -o APT::FTPArchive::Release::Architectures="${architectures[*]}" \
    -o APT::FTPArchive::Release::Components="$COMPONENT" \
    -o APT::FTPArchive::Release::Description="Tailscale client and panel applet for COSMIC" \
    release "dists/$SUITE" > Release.tmp
mv Release.tmp "dists/$SUITE/Release"

sign --clearsign --output "dists/$SUITE/InRelease" "dists/$SUITE/Release"
sign --detach-sign --armor --output "dists/$SUITE/Release.gpg" "dists/$SUITE/Release"

export_key=(gpg --batch --export)
[[ -n "${SIGNING_KEY:-}" ]] && export_key+=("$SIGNING_KEY")
"${export_key[@]}" > "$keyring_file"
if [[ ! -s "$keyring_file" ]]; then
    echo "no public key to export" >&2
    exit 1
fi

cat > "$NAME.sources" <<SOURCES
Types: deb
URIs: $base_url
Suites: $SUITE
Components: $COMPONENT
Signed-By: /usr/share/keyrings/$keyring_file
SOURCES

cat > index.html <<HTML
<!doctype html>
<meta charset="utf-8">
<title>cosmic-tailscale apt repository</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem}pre{background:#f3f3f3;padding:1rem;overflow-x:auto}</style>
<h1>cosmic-tailscale apt repository</h1>
<p>Packages for Pop!_OS 24.04, Ubuntu 24.04 and later, and Debian 13 and later, on
${architectures[*]}.</p>
<pre>sudo curl -fsSLo /usr/share/keyrings/$keyring_file $base_url/$keyring_file
sudo curl -fsSLo /etc/apt/sources.list.d/$NAME.sources $base_url/$NAME.sources
sudo apt update
sudo apt install $NAME</pre>
<p>Tailscale itself comes from <a href="https://tailscale.com/download/linux">Tailscale's own repositories</a>.
Source and issues: <a href="https://github.com/leechristophermurray/tailscale-cosmic-client">GitHub</a>.</p>
HTML

echo "$out"
