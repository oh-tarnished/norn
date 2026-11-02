#!/usr/bin/env python3
"""Renders a terminal transcript, read from stdin, to an SVG on stdout.

usage: render_svg.py TITLE < transcript > image.svg

A line starting `$ ` is a command and is drawn in the prompt colour. Nothing is
added to or changed in the text: this only draws what capture.sh recorded.
Standard library only.
"""
import sys
from xml.sax.saxutils import escape

FONT = "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace"
SIZE, LINE, CHAR, PAD = 14, 20, 8.6, 16
BACKGROUND, TEXT, PROMPT, MUTED = "#0d1117", "#e6edf3", "#7ee787", "#8b949e"


def main():
    title = sys.argv[1]
    lines = sys.stdin.read().rstrip("\n").split("\n")
    longest = max(len(title), *(len(line) for line in lines))
    width = int(longest * CHAR + 2 * PAD)
    height = (len(lines) + 2) * LINE + PAD
    out = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}"'
        f' viewBox="0 0 {width} {height}" role="img" aria-label="{escape(title)}">',
        f'<rect width="{width}" height="{height}" rx="8" fill="{BACKGROUND}"/>',
        f'<g font-family="{FONT}" font-size="{SIZE}" xml:space="preserve">',
        f'<text x="{PAD}" y="{LINE + 4}" fill="{MUTED}">{escape(title)}</text>',
    ]
    for number, line in enumerate(lines, start=3):
        colour = PROMPT if line.startswith("$ ") else TEXT
        y = number * LINE
        out.append(f'<text x="{PAD}" y="{y}" fill="{colour}">{escape(line)}</text>')
    out += ["</g>", "</svg>"]
    print("\n".join(out))


if __name__ == "__main__":
    main()
