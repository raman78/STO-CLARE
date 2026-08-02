# STO-CLARE

**C**ombat **L**og **A**nalyzer **Re**Mastered — a desktop tool that reads the
combat log Star Trek Online writes and turns it into tables and charts: damage,
healing, hits, kills, per-ability breakdowns, a live overlay, and uploads to the
OSCR ladder. It runs on Linux and Windows.

STO-CLARE grew out of
[AnotherNathan/STO_CombatLogAnalyzer](https://github.com/AnotherNathan/STO_CombatLogAnalyzer)
and everything that tool does is still here. It was renamed in version 2.0
because it has gone its own way — the original author is not involved in it and
is not the person to ask about it. The original copyright notice stays in the
licence files, where it belongs.

If you are coming from the older version: your settings are carried over the
first time you start STO-CLARE, and your old installation keeps working.

## What you need before you start

- Star Trek Online, and a character you have fought something with.
- Combat logging switched on in the game (step 2 below).
- Nothing else — STO-CLARE is a single program with no separate runtime.

## Quick start

1. Install it (see [Install](#install)).
2. In the game's chat window, type `/Combatlog 1` and press Enter. This has to
   be done again after every login.
3. Fight something.
4. Start STO-CLARE, open **Settings** and enter the path to the game's
   `combatlog.log`. It sits in
   `<your STO installation>\Star Trek Online\Live\logs\GameClient\`.
5. Click **Ok** at the bottom of the settings window, then the refresh button.

Your combats appear in the list at the top. Pick one and the tabs below fill in.

```
┌─ STO-CLARE V2.0.0 ──────────────────────────────────────────┐
│  [ Refresh ]  [ Settings ]  [ Overlay ]  [ Compare ]        │
│  Combat: [ [TFO] Hive Onslaught [Elite]  17:22 - 17:29  ▼ ] │
│  Type: [ Space ▼ ]   Level: [ Elite ▼ ]   Map: [ any ▼ ]    │
├─────────────────────────────────────────────────────────────┤
│  Summary │ Damage Dealt │ Damage Taken │ Healing Ally │ …   │
├─────────────────────────────────────────────────────────────┤
│  Name              DPS       Total Damage    Hits           │
│  ▸ You             182 456   41 963 220      3 412          │
│  ▸ Teammate        119 003   27 380 991      2 884          │
├─────────────────────────────────────────────────────────────┤
│  (charts for the selected rows)                             │
└─────────────────────────────────────────────────────────────┘
```

Tip: add the launch option `-NoAutoRotateLogs` to the game (on Steam:
right-click Star Trek Online → Properties → Launch Options) so it keeps writing
a single log file instead of splitting it. Here is a
[step-by-step guide](https://www.sto-league.com/how-to-disable-automatically-rotated-log-files/).
On Linux you can skip this — if the log does get split, STO-CLARE merges the
pieces back together for you.

---

## Install

### Linux — one command

Paste this into a terminal. It fetches the latest release, puts `sto-clare` on
your PATH and adds an applications-menu entry:

```sh
curl -fsSL https://raw.githubusercontent.com/raman78/STO-CLARE/main/install.sh | sh
```

To update later, run `sto-clare --upgrade` (or the same one-liner again).
`sto-clare --version` tells you what you have.

### Windows

Download and run the installer (`…-setup.exe`) from the Releases page. Update
with `sto-clare --upgrade` or by running a newer installer.

### From source

Install the Rust toolchain from [rust-lang.org](https://www.rust-lang.org/),
then build the release binary:

```sh
cargo build --release
```

---

## What STO-CLARE adds over the original

Everything from the original tool is still here; the tables below list what has
been added or fixed since. The "Offered back" column says whether the change was
also proposed to the original project — a number is a pull request there, a dash
means it lives here only.

### Reading your combats

| Feature | What it does | Offered back |
|---|---|---|
| Compare Combats | Pick several combats — from any log in a folder — and see them side by side. The damage breakdown is lined up group by group, with green and red numbers showing how much better or worse each combat was than the first one. You choose which columns to compare, and any ability can be charted across all of them. All of the compared combats open on the same player, so the differences are one player's runs rather than several people's. | — |
| Where a DPS difference came from | A DPS difference on its own hides what changed: firing more often while each hit lands softer can add up to almost nothing. Switch on the breakdown in the Columns menu and each difference is split into the part that came from landing more often and the part that came from each hit landing harder. The two always add up to the whole difference. | — |
| Healing that adds up | Healing is split into three tabs that do not overlap: what you healed on other people, what they healed on you, and what you healed on yourself. Healing yourself used to be counted in two places at once, and on a normal run one gear proc was most of it — which buried everything your team actually did. | — |
| Attacks are no longer counted as healing | Some abilities write the shield half of an attack in the same shape the game uses for shield repairs, so shooting an enemy showed up as healing it. On a normal run more than half of the healing listed as done to others was really damage. | — |
| Hull and shield side by side | Damage, hits, healing and heal ticks can show their hull and shield halves as columns of their own instead of only in a hover box, so you can see how much of an ability went where. Turn it off under Settings → General to get the compact table back. | — |
| Honest Resistance % | The Resistance column now measures what it says: how much the target's hull soaked up. It used to mix in the damage dealt to the target's shields, which a different stat stops entirely, and so read far too favourably. | — |
| Automatic map and difficulty | A combat is named after what actually happened in the fight instead of being left as "Combat". You get the map, tagged as a TFO or a patrol, together with its Advanced or Elite level — for example "[TFO] Hive Onslaught [Elite]". Your own naming rules still decide the base name and no longer have to add the level themselves. | — |
| Your own detection rules | A rules file next to your settings can adjust how maps and difficulties are recognised, so you do not have to wait for a new version when the game changes. | — |
| Correct average non-critical hit | Fixes a wrong average on runs with abilities that scored criticals on shields. | — |

### The combats list and clearing the log

| Feature | What it does | Offered back |
|---|---|---|
| Choose what to delete | "Clear Log File" opens a list of every combat with checkboxes, so you delete exactly the ones you mean to. Select all or none, and everything but the newest is ticked for you. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| List loads by itself | The combats list fills in when the app starts, without pressing "Refresh Now". | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Longer list | About 15 combats are shown at once, and the list scrolls when there are more. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Narrow the list | Under the toolbar, three menus filter the combats by type (space or ground), by level and by map. Each menu only offers what the others leave, so you cannot pick a combination that shows nothing, and a "Clear filter" button appears once anything is set. The Compare view filters exactly the same way. | — |
| No pointless refreshing | With auto refresh on, nothing reloads while the log is unchanged, so an expanded damage breakdown stays open while you look through it. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| The oldest combat works too | The first combat in a log can now be saved and deleted like any other. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Your place is kept | Opening "Clear Log File" no longer jumps you to the newest combat — the one you were reading stays open while the delete list refreshes. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Merged split logs (Linux) | If the game splits the combat log into many files, they are merged back into one so all your combats show up together. The originals are only removed once the merged log has been checked byte for byte, so nothing is lost. | — |

### The overlay

| Feature | What it does | Offered back |
|---|---|---|
| Stays above the game (Linux) | On Linux the overlay keeps sitting on top of the game, including in full screen, which was not possible before. | [#6](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/6) |
| Works in every Linux session | The Overlay button also works outside of a Wayland session. Over a full-screen game it then depends on your window manager, so a Wayland session stays the reliable one. | [#6](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/6) |
| Controls on the overlay | The move and column-picker buttons sit on the overlay itself rather than in the main window. | [#6](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/6) |
| Remembers where you left it | Its position is kept between sessions, and it matches the colours of the main window. | [#6](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/6) |
| No crash when opening it | Switching the overlay on used to close the whole program on Linux/Wayland. | [#9](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/9) |
| Shows data straight away | The newest combat appears the moment you open the overlay, and the list keeps up while the overlay runs. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Uses less memory | The overlay shares the main window's graphics device instead of setting up a second one. | [#6](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/6) |

### Windows and settings

| Feature | What it does | Offered back |
|---|---|---|
| The window remembers itself | The main window opens at the size you left it, and comes back maximised if you closed it that way. It also follows your mouse smoothly while you resize it, and cannot be shrunk so far that its controls no longer fit. | [#8](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/8) |
| Correct size at any UI scale | With the interface scale set to anything other than 100%, the remembered window no longer shrank a little on every start. | [#8](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/8) |
| Resizable Settings window | The Settings window can be made as tall as you like, stays on the screen when a section is expanded, and remembers its size. | — |
| Browse opens where you were | The file dialog for the combat log starts in the folder you last picked one from. | — |
| Settings kept with your account | Your settings and the log file are written to the place your system keeps program settings, so the tool also works when it is installed somewhere you cannot write to. Settings from older versions are picked up automatically. | — |
| Names with accents | Names containing non-English characters are shown correctly. | — |
| Scroll bars stay out of the way | A scroll bar no longer grows over the bottom row of a table when your pointer comes near it. | — |

### Installing and updating

| Feature | What it does | Offered back |
|---|---|---|
| One-command install (Linux) | A single command fetches the latest release, puts the program on your path and adds a menu entry. | — |
| Windows installer | A regular setup program instead of unpacking an archive by hand. | — |
| Update from inside the app | `sto-clare --upgrade` fetches and installs the newest release, and `sto-clare --version` tells you what you have. | — |
| Menu entry | The tool registers itself with your desktop, so you can start it from the applications menu. | — |

---

## Advanced settings

These live under **Settings → Analysis**. You do not need them to read your
damage — they change how rows are grouped in the tables.

### Indirect source grouping reversal

Some damage and healing does not come straight from you: pets, anomalies,
consoles that spawn something. Those show up as a row you can expand, with the
individual effects underneath.

Sometimes you want it the other way round — the effect on top, the pets
underneath. That is what a reversal rule does. The Tachyon Net Drones console
is the classic case: by default its effect is scattered over many rows, and one
rule folds it into a single row you can expand. The default settings ship with
a ready-made example for the starship trait Spore-Infused Anomalies; tick its
"on" box to use it.

### Custom grouping rules

A custom grouping rule folds several effects into one row. Useful for a weapon
with an extra proc — for example the Advanced Piezo Beam Array, whose Technical
Overload fires alongside the beam itself. The default settings include an
example for the Dark Matter Quantum Torpedo, again switched on with its "on"
box.

---

## Common situations

| If you want to… | Do this |
|---|---|
| See only Elite runs of one map | Use the Type / Level / Map menus under the toolbar. |
| Compare two runs of the same map | Open **Compare**, tick the combats, and read the green/red differences. |
| Watch your DPS while playing | Open **Overlay**. On Linux a Wayland session keeps it above a full-screen game. |
| Look at one ability over time | Click its row in the table; the charts below follow your selection. |
| Free up disk space | Use **Clear Log File** and tick the combats you no longer need. |
| Start fresh | Delete the settings file from the folder listed in the FAQ below. |

## What can go wrong

| Symptom | Likely cause | What to do |
|---|---|---|
| The combats list is empty | Combat logging is off in the game | Type `/Combatlog 1` in the game chat, fight something, then press refresh. |
| Still empty after that | The path to the log file is wrong | Settings → the combat log path must end in `combatlog.log`. |
| Only your newest fights show up | The game split the log into several files | Add `-NoAutoRotateLogs` to the launch options. On Linux the pieces are merged for you. |
| The overlay is behind the game | Your session is X11, or the window manager decides otherwise | Use a Wayland session, or run the game in windowed mode. |
| `sto-clare --upgrade` fails to write | The program is installed where your account cannot write | Reinstall with the one-liner above, which installs under your home folder. |
| Numbers look far too low for a run | You are reading a combat that was cut short in the log | Pick the neighbouring entry in the combats list; long fights can span two. |

## FAQ

**Q: I used STO_CombatLogAnalyzer. Do I have to set everything up again?**
A: No. The first time STO-CLARE starts it copies your old settings — including
your naming and grouping rules — into its own folder. The old installation is
left untouched, so it keeps working if you want to go back.

**Q: Where are my settings kept?**
A: In the folder your system uses for program settings: `~/.config/STO-CLARE`
on Linux, `%APPDATA%\STO-CLARE` on Windows.

**Q: Is this the same program as the original?**
A: It started as a copy of it and still does everything it does, but it is
developed separately now. Bug reports belong here, not with the original
author.

**Q: Does it change anything in the game?**
A: No. It only reads the log file the game writes.

## Where to get more help

- Report a problem or ask a question in the
  [issue tracker](https://github.com/raman78/STO-CLARE/issues).
- Every release is listed with its changes in `CHANGELOG.md`.
