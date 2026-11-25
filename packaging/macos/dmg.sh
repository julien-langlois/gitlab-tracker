#!/bin/sh
# Builds gitlab-tracker-<version>-<target>.dmg: "GitLab Tracker.app" (the binary,
# its icon and Info.plist) beside a link to /Applications, to drag it onto.
#
#   sh packaging/macos/dmg.sh <binary> <version> <target> <out-dir>
#
# Not notarized (no Apple Developer ID yet): the bundle is signed ad hoc, and the
# first launch needs Privacy & Security → Open Anyway (right click → Open before
# macOS 15), or `xattr -d com.apple.quarantine`; see docs/desktop-window.md.
set -eu
bin=$1 version=$2 target=$3 out=$4
root=$(cd "$(dirname "$0")/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

app="$work/GitLab Tracker/GitLab Tracker.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/gitlab-tracker"
cp "$root/assets/icon/gitlab-tracker.icns" "$app/Contents/Resources/"
# Info.plist versions are numbers only: v1.2.3-rc.1 → 1.2.3.
number=${version#v}
number=${number%%-*}
sed "s/@VERSION@/$number/g" "$root/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
codesign --force --deep --sign - "$app"
ln -s /Applications "$work/GitLab Tracker/Applications"

mkdir -p "$out"
file="$out/gitlab-tracker-$version-$target.dmg"
hdiutil create -volname "GitLab Tracker" -srcfolder "$work/GitLab Tracker" \
    -fs HFS+ -format UDZO -ov "$file"
echo "$file"
