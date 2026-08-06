#!/usr/bin/env python3
"""Make an anonymised combat log to take screenshots against.

Copies the tail of a real log and renames every player in it, so pictures for
the readme and the manual can show real numbers without showing real people —
yours or the strangers you happened to play with.

    ./demo/make-demo-log.py ~/path/to/combatlog.log /tmp/games

Writes <out>/Star Trek Online/Live/logs/GameClient/combatlog.log, which is what
you then point the program at (see settings.py).
"""

import pathlib
import re
import sys

# Character names are replaced in the order they are met: Alpha, Bravo, ...
CALL_SIGNS = [
    "Alpha", "Bravo", "Charlie", "Delta", "Echo", "Foxtrot", "Golf", "Hotel",
    "India", "Juliett", "Kilo", "Lima", "Mike", "November", "Oscar", "Papa",
    "Quebec", "Romeo", "Sierra", "Tango", "Uniform", "Victor", "Whiskey",
    "Xray", "Yankee", "Zulu",
]

# "<display>,P[accountid@userid CharName@handle#instance]". The display name
# holds no comma and no colon, which is what keeps the leading timestamp — full
# of colons — out of the match. Getting this wrong eats the timestamps and the
# program then reads nothing at all.
PLAYER = re.compile(r"([^,:]*),P\[(\d+)@(\d+) ([^\]#]+)(#\d+)?\]")

# How much of the end of the log to keep. Enough for a couple of dozen combats.
TAIL_BYTES = 45_000_000


def main() -> int:
    if len(sys.argv) != 3:
        print(__doc__)
        return 2
    source, out_root = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
    out = out_root / "Star Trek Online/Live/logs/GameClient/combatlog.log"
    out.parent.mkdir(parents=True, exist_ok=True)

    seen: dict[str, tuple[str, str, int, int]] = {}

    def rename(match: re.Match[str]) -> str:
        _display, _account, _user, full, instance = match.groups()
        if full not in seen:
            i = len(seen)
            base = CALL_SIGNS[i % len(CALL_SIGNS)]
            if i >= len(CALL_SIGNS):
                base += str(i // len(CALL_SIGNS) + 1)
            seen[full] = (base, f"{base}@player{i + 1}", 1_000_000 + i + 1, 2_000_000 + i + 1)
        display, handle, account, user = seen[full]
        return f"{display},P[{account}@{user} {handle}{instance or ''}]"

    with source.open("rb") as raw:
        raw.seek(max(0, source.stat().st_size - TAIL_BYTES))
        raw.readline()  # drop the partial first line
        text = raw.read().decode("utf-8", errors="replace")

    out.write_text("".join(PLAYER.sub(rename, line) for line in text.splitlines(keepends=True)))
    print(f"{len(seen)} players renamed -> {out}")

    leftovers = [m for m in re.findall(r"@[A-Za-z0-9_]+", out.read_text()) if not m.startswith(("@player", "@2"))]
    print("real handles left:", len(leftovers))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
