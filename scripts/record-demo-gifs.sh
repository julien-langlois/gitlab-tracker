#!/usr/bin/env bash
# Regenerates the two animated demos from the demo data (--demo), in English:
#   assets/gui.gif   the desktop window, through its built-in tour
#                    (GITLAB_TRACKER_GUI_TOUR, debug builds: one picture per step)
#   assets/demo.gif  the terminal UI, driven in tmux and rendered by ansi2gif.py
#
#   scripts/record-demo-gifs.sh            # both
#   scripts/record-demo-gifs.sh gui|tui    # one
#
# Needs tmux, python3 with Pillow, DejaVu Sans Mono (and Noto Color Emoji for the
# terminal's emoji), and a graphical session for the window. The user's own
# configuration is never read: each run gets a scratch XDG_CONFIG_HOME.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
what=${1:-both}
work=$(mktemp -d)
trap 'rm -rf "$work"; tmux kill-session -t gt-demo 2> /dev/null || true' EXIT
export LANGUAGE=en LC_ALL=C.UTF-8 LANG=C.UTF-8
cd "$root"

gui() {
    cargo build -p gitlab-tracker --features gui,stats
    mkdir -p "$work/gui-config" "$work/tour"
    XDG_CONFIG_HOME="$work/gui-config" GITLAB_TRACKER_GUI_SIZE=1600x900 \
        GITLAB_TRACKER_GUI_TOUR="$work/tour" \
        target/debug/gitlab-tracker --demo gui < /dev/null
    # Written aside then moved: an old GIF may belong to another user (root).
    python3 - "$work/tour" "$work/gui.gif" << 'EOF'
import sys
from pathlib import Path
from PIL import Image
shots = sorted(Path(sys.argv[1]).glob("*.ppm"))
if not shots:
    sys.exit("the tour wrote no picture")
frames = [Image.open(p).convert("RGB").resize((1280, 720), Image.LANCZOS) for p in shots]
frames = [f.convert("P", palette=Image.ADAPTIVE, colors=128) for f in frames]
frames[0].save(sys.argv[2], save_all=True, append_images=frames[1:],
               duration=2200, loop=0, optimize=True)
print(f"assets/gui.gif: {len(frames)} frames")
EOF
    mv -f "$work/gui.gif" assets/gui.gif
}

tui() {
    cargo build -p gitlab-tracker --release --no-default-features --features notifications,stats
    mkdir -p "$work/tui-config" "$work/frames"
    tmux kill-session -t gt-demo 2> /dev/null || true
    tmux new-session -d -s gt-demo -x 180 -y 46 \
        "XDG_CONFIG_HOME=$work/tui-config LANGUAGE=en LC_ALL=C.UTF-8 LANG=C.UTF-8 \
         $root/target/release/gitlab-tracker --demo"
    sleep 4
    n=0
    shot() { # [hold ms]: capture the pane as the next frame
        n=$((n + 1))
        tmux capture-pane -t gt-demo -e -p > "$work/frames/$(printf %03d "$n")${1:+@$1}.ansi"
    }
    key() { # key…, then a frame
        tmux send-keys -t gt-demo "$@"
        sleep 0.6
        shot
    }
    shot 2500
    key Tab                          # the Inspector
    for _ in 1 2 3 4 5 6; do key Down; done
    key p; shot 1500                 # pipelines
    key Down; key Down
    key p; key Tab                   # back to the list
    key Down; key Down; key Down
    key s; key s; key S; shot 1500   # sorting
    key Space; key Down; key Space   # flags
    key f; key Down; key Enter; shot 1500
    key f; key Up; key Enter
    key c; key Down; key Space; key Down; key Space; shot 1200
    key Enter; shot 2000
    tmux kill-session -t gt-demo
    python3 scripts/ansi2gif.py "$work/demo.gif" 700 "$work"/frames/*.ansi
    mv -f "$work/demo.gif" assets/demo.gif
    echo "assets/demo.gif: $n frames"
}

case $what in
    gui) gui ;;
    tui) tui ;;
    both) gui && tui ;;
    *) echo "usage: $0 [gui|tui|both]" >&2; exit 2 ;;
esac
