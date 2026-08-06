#!/usr/bin/env python3
"""Line up the columns in every markdown table of the given files.

    ./demo/format-md-tables.py README.md MANUAL.md docs/*.md

Changes only how the source reads; the rendered output is identical. Tables
inside fenced code blocks are left alone.
"""

import pathlib, re, sys

def width(s):  # em dashes and arrows are single columns; good enough for alignment
    return len(s)

def format_file(path):
    lines = pathlib.Path(path).read_text().split('\n')
    out, i, in_code, changed = [], 0, False, 0
    while i < len(lines):
        line = lines[i]
        if line.lstrip().startswith('```'):
            in_code = not in_code
            out.append(line); i += 1; continue
        # a table = a header row followed by a separator row
        if (not in_code and line.startswith('|') and i + 1 < len(lines)
                and re.fullmatch(r'\|(\s*:?-+:?\s*\|)+', lines[i + 1].strip())):
            block = []
            while i < len(lines) and lines[i].startswith('|'):
                block.append(lines[i]); i += 1
            rows = [[c.strip() for c in r.strip().strip('|').split('|')] for r in block]
            cols = max(len(r) for r in rows)
            rows = [r + [''] * (cols - len(r)) for r in rows]
            widths = [max(width(r[c]) for n, r in enumerate(rows) if n != 1) for c in range(cols)]
            widths = [max(w, 3) for w in widths]
            new = []
            for n, r in enumerate(rows):
                if n == 1:
                    new.append('|' + '|'.join('-' * (w + 2) for w in widths) + '|')
                else:
                    new.append('| ' + ' | '.join(c.ljust(widths[k]) for k, c in enumerate(r)) + ' |')
            if new != block:
                changed += 1
            out.extend(new)
            continue
        out.append(line); i += 1
    pathlib.Path(path).write_text('\n'.join(out))
    return changed

total = 0
for p in sys.argv[1:]:
    n = format_file(p)
    print(f"{p}: {n} table(s) reformatted")
    total += n
