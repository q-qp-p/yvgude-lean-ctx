Output "assets/leanctx-pr-pack.gif"

Require lean-ctx
Require git

Set Theme "GitHub Dark"
Set FontSize 24
Set Width 1200
Set Height 700
Set Framerate 30
Set Padding 24
Set BorderRadius 10
Set WindowBar "ColorfulRight"
Set WindowBarSize 36
Set TypingSpeed 45ms
Set LoopOffset 10%

Set Shell "bash"

Type "export PS1='leanctx$ '" Enter
Sleep 200ms

Type "./rust/target/release/lean-ctx pack --pr --depth 2" Enter
Sleep 2s

Type "./rust/target/release/lean-ctx pack --pr --json --depth 2" Enter
Sleep 2s

Type "git diff --name-status HEAD~1...HEAD | ./rust/target/release/lean-ctx pack --pr --diff-from-stdin --json --depth 2" Enter
Sleep 2s

