#!/bin/sh
# Builds gitlab-tracker-<version>-<arch>.AppImage from a release binary, with its
# desktop entry and icons. linuxdeploy copies in the libraries the binary links
# (D-Bus, OpenSSL), so the AppImage runs on distributions older than the build's;
# the graphics stack (OpenGL, Wayland, X11) stays the host's, as it must.
#
#   sh packaging/linux/appimage.sh <binary> <version> <out-dir>
#
# The architecture is the machine's (x86_64 or aarch64): run it on each.
set -eu
bin=$1 version=$2 out=$3
root=$(cd "$(dirname "$0")/../.." && pwd)
arch=$(uname -m)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Pinned: the "continuous" release changes under us.
tag=1-alpha-20251107-1
tool="$work/linuxdeploy-$arch.AppImage"
curl -fsSL -o "$tool" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/$tag/linuxdeploy-$arch.AppImage"
chmod +x "$tool"

app="$work/AppDir"
install -Dm755 "$bin" "$app/usr/bin/gitlab-tracker"
for n in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$root/assets/icon/png/gitlab-tracker-$n.png" \
        "$app/usr/share/icons/hicolor/${n}x$n/apps/gitlab-tracker.png"
done
install -Dm644 "$root/assets/icon/gitlab-tracker.svg" \
    "$app/usr/share/icons/hicolor/scalable/apps/gitlab-tracker.svg"

# The AppImage runs the binary itself: double-clicked, it has no terminal and
# opens the window (cli::window_without_terminal); from a shell, the terminal UI.
mkdir -p "$out"
out=$(cd "$out" && pwd)
file="$out/gitlab-tracker-$version-$arch.AppImage"
cd "$work"
# No FUSE on CI runners: linuxdeploy extracts itself instead.
APPIMAGE_EXTRACT_AND_RUN=1 LDAI_OUTPUT="$file" \
    "$tool" --appdir "$app" \
    --desktop-file "$root/assets/linux/gitlab-tracker.desktop" \
    --output appimage
echo "$file"
