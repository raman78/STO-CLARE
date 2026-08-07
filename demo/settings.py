#!/usr/bin/env python3
"""Build a throwaway settings folder pointing at the demo log.

    ./demo/settings.py /tmp/clare-demo LightDark [/tmp/games/.../combatlog.log]

Copies your live settings — so the naming and grouping rules come along and
combats get their proper names — then overrides the log path, the theme and
anything else that would put private data in a screenshot. Nothing is written
to your real settings folder.

Run the program against it with:

    XDG_CONFIG_HOME=/tmp/clare-demo ./target/release/sto-clare
"""

import datetime
import json
import pathlib
import shutil
import sys

APP_DIR = "STO-CLARE"
SETTINGS_FILE = "STO-CLARE_Settings.json"
DEFAULT_LOG = "/tmp/games/Star Trek Online/Live/logs/GameClient/combatlog.log"


def live_settings() -> pathlib.Path | None:
    for base in (pathlib.Path.home() / ".config", pathlib.Path.home() / "AppData/Roaming"):
        candidate = base / APP_DIR / SETTINGS_FILE
        if candidate.is_file():
            return candidate
    return None


# What the newest combats are labelled with in the pictures, newest first.
#
# More of them than any one picture shows, on purpose: the boundaries below are
# every gap in the log, while the program throws away the fights nobody dealt
# damage in (`Analyzer`, `retain(|combat| combat.total_damage_out.all > 0.0)`).
# Notes landing on those are simply never seen, so the list has to run past them
# for the combats a picture does show to carry one.
DEMO_NOTES = [
    "Cheops build",
    "FAW build",
    "torp boat, no buffs",
    "first run of the evening",
    "same build, no buffs",
    "cannon boat",
    "after the console swap",
    "warm-up run",
    "pug team",
    "solo, full uptime",
    "testing the new trait",
    "back to the old rotation",
]


def combat_starts(log: pathlib.Path, separation_seconds: float) -> list[datetime.datetime]:
    """When each combat in `log` began, oldest first.

    Mirrors `Analyzer::process_next_record`: a record more than the separation
    time after the last one starts a new combat, and a combat's start — which
    is what a note is keyed by (`CombatNotes::key_at`) — is the timestamp of
    its first record. The log writes `%y:%m:%d:%H:%M:%S.f` with one decimal;
    the parser pads that to milliseconds, so this does too.
    """
    starts: list[datetime.datetime] = []
    previous: datetime.datetime | None = None
    separation = datetime.timedelta(seconds=separation_seconds)
    with log.open(encoding="utf-8", errors="replace") as lines:
        for line in lines:
            stamp, _, _ = line.partition("::")
            try:
                time = datetime.datetime.strptime(stamp + "00", "%y:%m:%d:%H:%M:%S.%f")
            except ValueError:
                continue
            if previous is None or time - previous > separation:
                starts.append(time)
            previous = time
    return starts


def demo_notes(log: pathlib.Path, separation_seconds: float) -> dict[str, str]:
    if not log.is_file():
        return {}
    newest_first = list(reversed(combat_starts(log, separation_seconds)))
    return {
        start.strftime("%Y-%m-%d %H:%M:%S.") + f"{start.microsecond // 1000:03d}": note
        for start, note in zip(newest_first, DEMO_NOTES)
    }


def main() -> int:
    if len(sys.argv) < 3:
        print(__doc__)
        return 2
    root, theme = pathlib.Path(sys.argv[1]), sys.argv[2]
    log = sys.argv[3] if len(sys.argv) > 3 else DEFAULT_LOG

    source = live_settings()
    settings = json.loads(source.read_text()) if source else {}

    settings.setdefault("analysis", {})["combatlog_file"] = log
    settings["analysis"]["consolidate_combatlog"] = True
    settings.setdefault("visuals", {})["theme"] = theme
    settings["visuals"].setdefault("ui_scale", 1.0)
    # Made-up notes on the newest few combats, replacing the real ones: those
    # are the user's own words about their runs and must not reach a picture,
    # while the pictures of the combats list and of Compare have to show what a
    # note looks like. The shape matters: a bare {} fails to load and the
    # program then falls back to its defaults, log path and all.
    separation = settings.get("analysis", {}).get("combat_separation_time_seconds", 45.0)
    settings["combat_notes"] = {"notes": demo_notes(pathlib.Path(log), float(separation))}
    settings.setdefault("general", {})["overlay_shown"] = False
    settings["window"] = {"size": [1280.0, 720.0], "maximized": False}

    target = root / APP_DIR
    shutil.rmtree(root, ignore_errors=True)
    target.mkdir(parents=True)
    (target / SETTINGS_FILE).write_text(json.dumps(settings, indent=2))
    print(f"{target / SETTINGS_FILE} -> theme {theme}, log {log}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
