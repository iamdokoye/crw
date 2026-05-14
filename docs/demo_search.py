#!/usr/bin/env python3
"""Search the web via CRW API."""
from crw import CrwClient

client = CrwClient(api_url="http://localhost:3030")
results = client.search("best web scraper 2026", limit=3)

print("Search results for 'best web scraper 2026':\n")
for r in results["results"]:
    print(f"  • {r['title']}")
    print(f"    {r['url']}\n")
