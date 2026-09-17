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
#   apt/index.html                         how to add the repository
#
# The site root is shared with the project's page (scripts/build-site.sh), so
# this writes only the files above and never clears the directory.
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

mkdir -p apt
cat > apt/index.html <<HTML
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>cosmic-tailscale apt repository</title>
<link rel="icon" href="../icon.svg" type="image/svg+xml">
<link rel="stylesheet" href="../style.css">
<header class="site-header"><strong><a href="../">cosmic-tailscale</a></strong></header>
<main>
<div class="hero">
<h1>Apt repository</h1>
<p class="lead">Packages for Pop!_OS 24.04, Ubuntu 24.04 and later, and Debian 13 and
later, on ${architectures[*]}. Signed; the keyring below is what verifies them.</p>
<pre><code>sudo curl -fsSLo /usr/share/keyrings/$keyring_file \\
  $base_url/$keyring_file
sudo curl -fsSLo /etc/apt/sources.list.d/$NAME.sources \\
  $base_url/$NAME.sources
sudo apt update
sudo apt install $NAME</code></pre>
<p class="note">Tailscale itself comes from
<a href="https://tailscale.com/download/linux">Tailscale's own repositories</a>, so the
daemon keeps receiving its security updates.</p>
<p><a href="dists/$SUITE/Release">Release</a> ·
<a href="$NAME.sources">sources file</a> ·
<a href="$keyring_file">keyring</a> ·
<a href="https://github.com/leechristophermurray/tailscale-cosmic-client">source and issues</a></p>
</div>
</main>
HTML

echo "$out"
