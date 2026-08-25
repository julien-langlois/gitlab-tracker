# demo.tape — Stats overlay walkthrough
# Requires: vhs (https://github.com/charmbracelet/vhs)
# Run:      vhs demo.tape
# Output:   demo.gif (used in gitlab-tracker-stats/README.md)
#
# This tape opens the main TUI in demo mode, then demonstrates every
# feature of the Stats overlay: opening, scrolling, window cycling.
Output stats.gif

Set Width 1800
Set Height 900
Set FontSize 13
Set Padding 15

Set Theme "Catppuccin Macchiato"
Set TypingSpeed 50ms

Hide
Type "./target/release/gitlab-tracker --demo"
Enter
Sleep 4s
Show

# ── Step 1: Open the Stats overlay with [G] ──────────────────────────────────
# The overlay appears immediately with "Loading…" while the aggregation runs,
# then renders the full report once the async task completes.
Type "g"
Sleep 2s

# ── Step 2: Scroll through the report ────────────────────────────────────────
# Show the Throughput & Cycle Time section.
Sleep 1.5s

# Scroll down to the By Author section.
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Sleep 1.5s

# Scroll down to the Correlations section.
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms
Sleep 2s

# ── Step 3: Cycle the time window with [W] ───────────────────────────────────
# Each press triggers a fresh DB query and re-renders the report.
Type "w"
Sleep 2s
# Title now reads "Last 90 days" — show it briefly.
Sleep 1s

Type "w"
Sleep 2s
# Title now reads "Last 365 days".
Sleep 1s

Type "w"
Sleep 2s
# Title now reads "All time" — shows the full dataset.
Sleep 1.5s

# Cycle back to 30 days to end on the default.
Type "w"
Sleep 2s
Sleep 1s

# ── Step 4: Scroll back to top with [k] ──────────────────────────────────────
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Type "k"
Sleep 150ms
Sleep 1.5s

# ── Step 5: Close the overlay with [Esc] ─────────────────────────────────────
Escape
Sleep 1s

# Return to normal TUI browsing briefly before the gif loops.
Sleep 2s
