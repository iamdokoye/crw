# VHS Demo - fastCRW
Output docs/demo.gif

Set Shell zsh
Set FontSize 17
Set Width 950
Set Height 550
Set Theme "Catppuccin Mocha"
Set Padding 20
Set TypingSpeed 50ms

# Setup
Hide
Type "export PATH=/Users/us/coding/crw/crw-opencore/target/release:$PATH && cd /Users/us/coding/crw/crw-opencore/python && clear"
Enter
Show
Sleep 300ms

# 1. Scrape
Type "crw example.com"
Enter
Sleep 2500ms

# 2. Links
Type "crw news.ycombinator.com --format links | head -8"
Enter
Sleep 3500ms

# 3. Search
Type "uv -q run python ../docs/demo_search.py"
Enter
Sleep 5000ms

Sleep 1000ms
