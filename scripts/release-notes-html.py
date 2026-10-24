#!/usr/bin/env python3
"""Release notes for Sparkle's update dialog, as a small self-contained HTML
page (Sparkle shows it in a web view, so it must not be a GitHub page).

  release-notes-html.py <version> [<notes.md>]   < commit subjects on stdin

Uses release-notes/v<version>.md when given/present ("- item" lines, "## "
headings, blank lines between paragraphs); otherwise turns the commit
subjects into a list, dropping release plumbing ("Coo 0.1.2", "Site:", ...).
"""
import html, os, re, sys

ver = sys.argv[1]
path = sys.argv[2] if len(sys.argv) > 2 else f"release-notes/v{ver}.md"
SKIP = re.compile(r"^(Coo \d|Site[: ]|Release[: ]|Perf harness|Pin Rust|Merge |docs?:|CI[: ])", re.I)

def inline(t):
    t = html.escape(t)
    t = re.sub(r"\*\*(.+?)\*\*", r"<b>\1</b>", t)
    t = re.sub(r"`(.+?)`", r"<code>\1</code>", t)
    return t

body = []
if os.path.exists(path):
    items = []
    def flush():
        if items:
            body.append("<ul>" + "".join(f"<li>{i}</li>" for i in items) + "</ul>")
            items.clear()
    for line in open(path, encoding="utf-8"):
        line = line.rstrip()
        if line.startswith("- "):
            items.append(inline(line[2:]))
        elif line.startswith("## "):
            flush(); body.append(f"<h3>{inline(line[3:])}</h3>")
        elif line.strip():
            flush(); body.append(f"<p>{inline(line)}</p>")
        else:
            flush()
    flush()
else:
    subjects = [l.strip().lstrip("- ").strip() for l in sys.stdin if l.strip()]
    subjects = [s for s in subjects if not SKIP.match(s)]
    body.append("<ul>" + "".join(f"<li>{inline(s)}</li>" for s in subjects[:30]) + "</ul>")

print(f"""<!doctype html><html><head><meta charset="utf-8"><style>
:root {{ color-scheme: light dark; }}
body {{ font: 13px/1.5 -apple-system, BlinkMacSystemFont, sans-serif; margin: 10px 14px; color: #1d1d1f; }}
h2 {{ font-size: 15px; margin: 0 0 6px; }} h3 {{ font-size: 13px; margin: 12px 0 4px; }}
ul {{ margin: 0; padding-left: 18px; }} li {{ margin: 3px 0; }} p {{ margin: 6px 0; }}
code {{ font: 12px ui-monospace, Menlo, monospace; }}
@media (prefers-color-scheme: dark) {{ body {{ color: #f5f5f7; }} }}
</style></head><body><h2>What's new in Coo {html.escape(ver)}</h2>
{''.join(body)}
</body></html>""")
