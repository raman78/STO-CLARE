# STO_CombatLogAnalyzer (short CLA)
Tool to parse and analyze the combat log file from Star Trek Online.

It displays the result of this analysis in convenient tables and also provides multiple diagrams.
| Combat Summary                         | Outgoing Damage                       |
| -------------------------------------- | ------------------------------------- |
| ![Summary Tab](images/summary_tab.png) | ![Summary Tab](images/damage_tab.png) |

---
## Features in this fork

This is a fork of [AnotherNathan/STO_CombatLogAnalyzer](https://github.com/AnotherNathan/STO_CombatLogAnalyzer).
Everything from the original tool is still here — the table below lists only
what this fork adds on top of it.

The "Offered back" column says whether the change has been proposed to the
original project. A number is a pull request there; a dash means the feature
lives only in this fork for now.

### Reading your combats

| Feature | What it does | Offered back |
|---|---|---|
| Compare Combats | Pick several combats — from any log in a folder — and see them side by side. The damage breakdown is lined up group by group, with green and red numbers showing how much better or worse each combat was than the first one. You choose which columns to compare, and any ability can be charted across all of them. | — |
| Automatic map and difficulty | A combat is named after what actually happened in the fight instead of being left as "Combat". You get the map, tagged as a TFO or a patrol, together with its Advanced or Elite level — for example "[TFO] Hive Onslaught [Elite]". Your own naming rules still decide the base name and no longer have to add the level themselves. | — |
| Your own detection rules | A rules file next to your settings can adjust how maps and difficulties are recognised, so you do not have to wait for a new version when the game changes. | — |
| Correct average non-critical hit | Fixes a wrong average on runs with abilities that scored criticals on shields. | — |

### The combats list and clearing the log

| Feature | What it does | Offered back |
|---|---|---|
| Choose what to delete | "Clear Log File" opens a list of every combat with checkboxes, so you delete exactly the ones you mean to. Select all or none, and everything but the newest is ticked for you. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| List loads by itself | The combats list fills in when the app starts, without pressing "Refresh Now". | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
| Longer list | About 15 combats are shown at once, and the list scrolls when there are more. | [#7](https://github.com/AnotherNathan/STO_CombatLogAnalyzer/pull/7) |
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

### Installing and updating

| Feature | What it does | Offered back |
|---|---|---|
| One-command install (Linux) | A single command fetches the latest release, puts the program on your path and adds a menu entry. See [Install](#install). | — |
| Windows installer | A regular setup program instead of unpacking an archive by hand. | — |
| Update from inside the app | `sto-cla --upgrade` fetches and installs the newest release, and `sto-cla --version` tells you what you have. | — |
| Menu entry | The tool registers itself with your desktop, so you can start it from the applications menu. | — |

---
## Install

### Linux — one command (nothing to download by hand)
Paste this into a terminal. It fetches the latest release, puts `sto-cla` on
your PATH and adds an applications-menu entry:

```sh
curl -fsSL https://raw.githubusercontent.com/raman78/STO_CombatLogAnalyzer/main/install.sh | sh
```

Later, upgrade to the newest release with either:

```sh
sto-cla --upgrade      # updates the app in place
```

(or just run the one-liner above again). Check the installed version with
`sto-cla --version`.

### Windows
Download and run the installer (`…-setup.exe`) from the Releases page. Upgrade
with `sto-cla --upgrade` or by running a newer installer.

### From source
See “Building the tool from Source” below.

---
## Getting started
1. Install using one of the methods above (or download it from the Releases page).

2. **Recommended:** stop the game from splitting the combat log into many files.
   Add the launch option `-NoAutoRotateLogs` to the game (on Steam: right‑click
   Star Trek Online → Properties → Launch Options), so it always writes a single
   `combatlog.log`. See this [step‑by‑step guide](https://www.sto-league.com/how-to-disable-automatically-rotated-log-files/).

   On Linux you can skip this if you want — if the log still gets split,
   STO_CombatLogAnalyzer automatically merges the pieces back into one for you.

3. Go into the game and type "/Combatlog 1" into the chat window. You have to do
   this again every time you log in.
   
4. Fight something.

5. Start STO_CombatLogAnalyzer, open the Settings and enter the path to the combatlog file of the game located at "\<path to STO installation\>\Star Trek Online\Live\logs\GameClient\combatlog.log."
Click Ok at the bottom of the settings window.

6. Click the refresh button.

---
## Advanced Features
This paragraph describes the more advanced and less intuitive parts of the analyzer. The described features can be found in the Settings in the Analysis tab.

### Indirect Source Grouping Reversal Rules
Indirect sources are damage or heal sources that do not come directly from an entity but still belong to this entity. These indirect sources can for instance be pets, most anomalies and more.

Indirect sources show up in the damage or heal tables as rows that can be expanded, where the sub-rows show the actual effects that made damage or healed.

Sometimes it is more desirable to have the effect as row that can be expanded with the indirect sources as the sub-rows. This is where Indirect Source Grouping rules come into play.

Lets see this with at the example of the Tachyon Net Drones Console Ability. The effect of this console shows up as many different rows in the damage table by default. But with a Indirect Source Reversal Rule it becomes one single row in the table with the indirect sources as a sub-row.

![Tachyon Net Drones Indirect Source Grouping Reversal](images/tachyon_net_drones_indirect_source_grouping_reversal.png)

The default settings also come with an example for the starship trait Spore-Infused Anomalies, which you can simple activate by ticking the "on" checkbox.

### Custom Grouping Rules
Custom grouping rules allows you to group multiple effects into a single row in the damage or heal tables.

Here is an example of grouping up all effects of the Advanced Piezo Beam Array, which has the neat extra effect of the Technical Overload when used with Beam Overload or Surgical Strikes.

![Advanced Piezo Beam Array Custom Grouping](images/advanced_piezo_beam_custom_grouping_rule.png)

The default settings also come with an example for the Dark Matter Quantum Torpedo, which you can simple activate by ticking the "on" checkbox.

---
## Building the tool from Source
Install the rust tool chain from https://www.rust-lang.org/.

And the build it with

```
C:\path\to\STO_CombatLogAnalyzer> cargo build --release
```

And that is it.

