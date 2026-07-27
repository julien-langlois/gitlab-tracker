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

# --- MR 104 is row 0 at startup: switch to Inspector immediately ---
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

# --- Scroll back to top ---
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

Sleep 800ms

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

# --- Sort demo: cycle columns then toggle order ---
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "s"
Sleep 1s
Type "S"
Sleep 1.5s

Sleep 2s
