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
    # Notes are the user's own words about their runs — keep them out of
    # screenshots. The shape matters: a bare {} fails to load and the program
    # then falls back to its defaults, log path and all.
    settings["combat_notes"] = {"notes": {}}
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
