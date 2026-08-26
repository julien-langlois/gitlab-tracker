# stats.tape — Stats overlay walkthrough
# Requires: vhs (https://github.com/charmbracelet/vhs)
# Run:      vhs stats.tape
# Output:   stats.gif (used in gitlab-tracker-stats/README.md)
#
# Scenario: a team member opens the stats overlay after a busy sprint.
# The walkthrough covers all five report sections:
#   1. Throughput & Cycle Time
#   2. Open MR Backlog
#   3. Cycle Time by Author / Reviewer
#   4. Spearman Correlations
#   5. Poisson Insights (forecasts, queue model, anomaly signals)   ← new
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

# ── Step 1: Open the Stats overlay ───────────────────────────────────────────
# [G] triggers the async aggregation. A spinner is shown while the DB is
# queried; the full report renders once all five sections are ready.
Type "g"
Sleep 2s

# ── Step 2: Throughput & Cycle Time section ───────────────────────────────────
# Visible immediately at the top: merged count, throughput/week, median and
# P90 cycle times, avg diff size, avg comments, pipeline failure rate.
Sleep 2s

# ── Step 3: Scroll to the Open MR Backlog section ────────────────────────────
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 1.5s

# ── Step 4: Scroll to the Cycle Time by Author / Reviewer sections ───────────
# Horizontal bar charts show who has the longest review cycles.
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 1.5s

# ── Step 5: Scroll to the Spearman Correlations section ──────────────────────
# Colour-coded rows: ● Cyan = very strong, ● Yellow = moderate, · = negligible.
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 2s

# ── Step 6: Scroll to the Poisson Insights section ───────────────────────────
# This section has three sub-sections rendered in sequence.

# 6a. Throughput forecasts: probability bars for 1-week and 2-week targets.
#     Green ≥ 75% / Yellow 40–74% / Red < 40%.
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 2s

# 6b. Queue model (M/M/1): traffic intensity ρ, expected MRs in system,
#     and expected wait time for the next MR.
#     ρ is colour-coded: Green < 0.60, Yellow 0.60–0.79, Red ≥ 0.80.
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 2s

# 6c. Anomaly signals: pipeline failures and comment volume vs Poisson baseline.
#     ✔ Normal · ↑ Elevated · ⚠ Warning · ✘ Critical
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms
Sleep 2.5s

# ── Step 7: Cycle the time window with [W] ───────────────────────────────────
# Each press triggers a full DB re-query and re-renders all five sections,
# including updated Poisson insights for the new window's λ.
Type "w"
Sleep 2s
# Header now reads "Last 90 days".
Sleep 1s

Type "w"
Sleep 2s
# Header now reads "Last 365 days".
# Larger sample → Poisson forecasts are more stable; queue insight may differ.
Sleep 1.5s

Type "w"
Sleep 2s
# Header now reads "All time".
Sleep 1.5s

# Cycle back to the default 30-day window.
Type "w"
Sleep 2s
Sleep 1s

# ── Step 8: Scroll back to the top ───────────────────────────────────────────
# Use Page Up to jump quickly back to the Throughput & Cycle Time section.
PageUp
Sleep 400ms
PageUp
Sleep 400ms
PageUp
Sleep 400ms
Sleep 1.5s

# ── Step 9: Close the overlay ────────────────────────────────────────────────
Escape
Sleep 1s

# Brief pause on the main MR table before the gif loops.
Sleep 2s
