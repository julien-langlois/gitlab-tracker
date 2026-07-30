# demo.tape
Output demo.gif

Set Width 1800
Set Height 900
Set FontSize 13
Set Padding 15

Set Theme "Catppuccin Macchiato"
Set TypingSpeed 50ms

Hide
Type "./target/release/gitlab-tracker --demo"
Enter
Sleep 3s
Show

# --- MR 104 is row 0 at startup (flagged ★): open the Inspector ---
Tab
Sleep 800ms

# --- Scroll down through the long MR description ---
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
Down
Sleep 250ms
Down
Sleep 250ms
Down
Sleep 250ms

# --- Pause so the viewer can read the bottom of the description ---
Sleep 1.5s

# --- Scroll back to top to show State / Merge status badges ---
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms
Up
Sleep 200ms

Sleep 1s

# --- Switch to Pipeline view with [P] ---
Type "p"
Sleep 1.5s

# --- Scroll through the pipeline jobs list ---
Down
Sleep 300ms
Down
Sleep 300ms
Down
Sleep 300ms

Sleep 1s

# --- Switch back to MR Info view with [P] ---
Type "p"
Sleep 1s

# --- Switch back to Dashboard pane ---
Tab
Sleep 800ms

# --- Navigate a few rows to show the list ---
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms

# --- Sort demo: cycle sort column then toggle order ---
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "S"
Sleep 1.5s

Sleep 800ms

# --- Flag / unflag demo: Space toggles the flag on the selected MR ---
Space
Sleep 800ms
Space
Sleep 800ms

# --- Navigate to another MR and flag it ---
Down
Sleep 400ms
Space
Sleep 800ms

# --- Filter demo: [F] cycles All → Flagged ★ → All ---
Type "f"
Sleep 1.5s
Type "f"
Sleep 1s

# --- Column picker demo: [C] opens the picker, toggle target_branch and labels ---
Type "c"
Sleep 1s
Down
Sleep 400ms
Enter
Sleep 600ms
Down
Sleep 400ms
Enter
Sleep 600ms
Type "c"
Sleep 1s

Sleep 2s
