# STO_CombatLogAnalyzer (short CLA)
Tool to parse and analyze the combat log file from Star Trek Online.

It displays the result of this analysis in convenient tables and also provides multiple diagrams.
| Combat Summary                         | Outgoing Damage                       |
| -------------------------------------- | ------------------------------------- |
| ![Summary Tab](images/summary_tab.png) | ![Summary Tab](images/damage_tab.png) |

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

