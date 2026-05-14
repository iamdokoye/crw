# VHS Demo - fastCRW
Output docs/demo.gif

Set Shell zsh
Set FontSize 16
Set Width 900
Set Height 500
Set Theme "Catppuccin Mocha"
Set Padding 20
Set TypingSpeed 40ms

# Setup - hide the PATH setup
Hide
Type "export PATH=/Users/us/coding/crw/crw-opencore/target/release:$PATH && clear"
Enter
Show
Sleep 500ms

# 1. Show install command (don't run it)
Type "# Install CRW (one-liner)"
Enter
Sleep 300ms
Type "curl -fsSL https://raw.githubusercontent.com/us/crw/main/install.sh | sh"
Sleep 1500ms
Enter
Sleep 500ms

# Clear and start demo
Type "clear"
Enter
Sleep 300ms

# 2. Scrape a website
Type "# Scrape any URL to markdown"
Enter
Sleep 300ms
Type "crw example.com"
Enter
Sleep 2500ms

# 3. Extract links
Type "# Extract links from a page"
Enter
Sleep 300ms
Type "crw news.ycombinator.com --format links | head -5"
Enter
Sleep 3000ms

# 4. Web search
Type "# Web search with SearXNG"
Enter
Sleep 300ms
Type 'crw search "rust web scraper" --limit 3'
Enter
Sleep 4000ms

Sleep 1500ms
