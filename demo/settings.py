#!/usr/bin/env python3
"""Build a throwaway settings folder pointing at the demo log.

    ./demo/settings.py /tmp/clare-demo LightDark [/tmp/games/.../combatlog.log]

Starts from the settings the program ships with and takes from yours only the
rules that decide how combats are named and grouped — without those, pictures
would show fights the program could not name. Nothing else of yours is copied.

It used to work the other way round: copy everything, then patch out what must
not be photographed. That leaked three times, each caught only by looking at the
picture — a note you had written, the mode Compare happened to be left in, the
path to your own game folder printed in full. A list of exceptions is only ever
as good as the last thing somebody remembered to add to it.

Run the program against it with:

    XDG_CONFIG_HOME=/tmp/clare-demo ./target/release/sto-clare
"""

import datetime
import json
import pathlib
import shutil
import sys

APP_DIR = "STO-CLARE"
DEFAULTS = "src/app/settings/STO-CLARE_Settings.json"
# What a picture needs from the live settings, and nothing besides: the rules
# that name and group combats.
CARRIED_OVER = (
    "combat_name_rules",
    "custom_group_rules",
    "indirect_source_grouping_revers_rules",
    "damage_out_exclusion_rules",
)
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

    # The program's own defaults, as shipped.
    defaults = pathlib.Path(__file__).resolve().parent.parent / DEFAULTS
    settings = json.loads(defaults.read_text())

    # The only thing worth having from the live settings: how combats get named
    # and grouped. Everything else there is personal by default.
    source = live_settings()
    if source:
        live = json.loads(source.read_text()).get("analysis", {})
        for rules in CARRIED_OVER:
            if rules in live:
                settings.setdefault("analysis", {})[rules] = live[rules]

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
    # The remembered log is a path on this machine — a home directory and a game
    # folder — and it is printed in full in the General settings. Pointed at the
    # demo log so the picture shows what the feature does without showing where
    # anybody lives.
    settings["general"]["default_combatlog_file"] = log
    settings["window"] = {"size": [1280.0, 720.0], "maximized": False}
    # Compare opens in the state the manual describes, not in whatever mode the
    # settings were copied from. Averages and the breakdown are shown in
    # pictures of their own, by pressing their buttons — a picture that arrives
    # already averaged contradicts the text next to it.
    settings["compare"] = {
        "columns": ["Dps", "Resistance", "Critical", "Accuracy"],
        "show_dps_breakdown": False,
        "show_averages": False,
    }

    target = root / APP_DIR
    shutil.rmtree(root, ignore_errors=True)
    target.mkdir(parents=True)
    (target / SETTINGS_FILE).write_text(json.dumps(settings, indent=2))
    print(f"{target / SETTINGS_FILE} -> theme {theme}, log {log}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
