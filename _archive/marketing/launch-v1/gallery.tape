# Product Hunt gallery stills — render with: vhs marketing/launch-v1/gallery.tape
# Produces three 1280x720 PNGs from real command output (no mockups).

Output marketing/launch-v1/gallery-tmp.gif

Set FontSize 18
Set Width 1280
Set Height 720
Set Theme "Catppuccin Mocha"
Set Padding 16
Set TypingSpeed 1ms

Env PATH "rust/target/release:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

Hide
Type "clear && lean-ctx gain --wrapped"
Enter
Sleep 2s
Show
Screenshot marketing/launch-v1/gallery-1-wrapped.png
Sleep 1s

Hide
Type "clear && lean-ctx doctor --migrate-check"
Enter
Sleep 2s
Show
Screenshot marketing/launch-v1/gallery-2-migrate.png
Sleep 1s

Hide
Type "clear && echo '366 lines -> the API surface your agent actually needs:' && lean-ctx read rust/src/core/contracts.rs --mode signatures | head -24"
Enter
Sleep 2s
Show
Screenshot marketing/launch-v1/gallery-3-signatures.png
Sleep 1s
