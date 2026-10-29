#!/bin/sh
# Installs the icon and the desktop entry for the current user, so the window
# gets its icon in the dock and can be pinned (Ubuntu: right click → Pin to Dash).
# Run from anywhere: sh assets/linux/install-desktop-entry.sh
set -eu
here=$(cd "$(dirname "$0")" && pwd)
icons="$here/../icon"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

for n in 16 24 32 48 64 128 256 512; do
    install -Dm644 "$icons/png/gitlab-tracker-$n.png" "$data/icons/hicolor/${n}x$n/apps/gitlab-tracker.png"
done
install -Dm644 "$icons/gitlab-tracker.svg" "$data/icons/hicolor/scalable/apps/gitlab-tracker.svg"

# The launcher does not read the shell's PATH: point Exec at the installed binary.
bin=$(command -v gitlab-tracker || true)
if [ -z "$bin" ]; then
    echo "gitlab-tracker is not on PATH: install it first (cargo install …)" >&2
    exit 1
fi
mkdir -p "$data/applications"
sed "s|^Exec=gitlab-tracker|Exec=$bin|" "$here/gitlab-tracker.desktop" \
    > "$data/applications/gitlab-tracker.desktop"

gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
update-desktop-database -q "$data/applications" 2>/dev/null || true
echo "Installed: $data/applications/gitlab-tracker.desktop (Exec=$bin gui)"
