# Change Log

# unreleased

### Other Changes
- when comparing runs, the note you wrote for a run now follows it through the whole comparison: it labels the run's line on the chart and sits under its column heading in the table, so you read "Cheops build" against "FAW build" instead of "#1" against "#2"
- a run's number and note in a comparison are printed in the colour of that run's line on the chart — in the list at the top and in every column heading — so a column and a line can be paired up at a glance

## v2.1.1

### Fixes
- the Overlay button switches the overlay off again. Since 2.1.0 it came straight back the instant it was closed, so the button looked like it did nothing

## v2.1.0

### Major Changes
- there is now a manual: a walk through every part of the program with pictures — reading a run, comparing two of them, the overlay, the ladder and every setting — linked from the readme
- the program has been given a coat of paint: rounded buttons and fields, a lit edge that firms up as you point at something, and shadows under the settings and popup windows. The colours are the ones you already know — every theme keeps its own — and fields still stand out from the page behind them as clearly as before
- two new themes to pick from under Settings → Visuals: **Nebula**, deep space blue with a cyan highlight, and **Frost Light**, a cool daylight look. The readme shows all five side by side
- the overlay carries its own buttons on every system now, not just on Linux: the column picker and the move handle sit on the overlay itself instead of in the main window, and you can still click straight through the rest of it at the game behind
- the overlay can be made see-through, so it stops covering a chunk of the game. Settings → Visuals has an **Overlay Opacity** slider; the figures stay solid however faint you make the background, so your DPS is still readable at a glance. It turns out the overlay was never actually see-through before: it asked to be, but the setting was being thrown away before it reached the screen

### Other Changes
- the readme now spells out how to bring your settings over from the original Combat Log Analyzer, step by step. Your naming rules, grouping rules, log path and the rest all come across, and your old installation is left working
- the icon shows up where the desktop puts it too: in the window's own frame, in the task switcher and on the taskbar. On a Wayland desktop it was a generic "unknown application" mark, because the entry the desktop keeps for the program was filed under a different name than the one the window gives — the two agree now
- the lettering on the icon is black, and the mark itself is no longer partly see-through — against a dark desktop it used to take on the colour behind it instead of standing on its own tile

## v2.0.1

### Other Changes
- the icon has a new look: the mark is now made of glass, lit from above, on a deep blue tile, with CLA written across the whole width so it can be read at a glance instead of squinting at a small caption

## v2.0.0

### Major Changes
- the program is now called **STO-CLARE** — Combat Log Analyzer ReMastered. It reads the same logs and does everything it did before; what changes is the name it goes by, the icon it shows and the fact that it is developed on its own from here on. It began as a fork of AnotherNathan's Combat Log Analyzer, which is said plainly in the readme and in both licence files, and that credit is not going anywhere
- your settings come with you. The first time the renamed program starts it copies everything it used to keep — the log path, your naming and grouping rules, window size, the lot — into its own folder. The old installation is left exactly as it was, so it still runs if you want to go back to it
- the program has an icon of its own: a black Starfleet delta on a deep blue field, over a red CLA nameplate. It replaces the one inherited from the original tool, in the window, on the taskbar, in the applications menu and in the Windows installer
- the menu entry, the taskbar name and the update command now read STO-CLARE. On Linux the entry left behind by the old name is cleared away once it no longer points at a program that is there; on Windows the setup replaces the old installation in place and removes the old shortcut. Updating from inside the program is `sto-clare --upgrade`

### Other Changes
- charts draw their series in a fixed set of eight colours, picked so that neighbouring ones stay apart for colour-blind readers as well, instead of colours worked out on the spot. Each theme has its own version of them, so the lines keep their bite on a light background as well as on a dark one
- the Light theme now colours the rest of what the program paints, not only its windows: the green and red differences in Compare, the warning marks in the settings and the little status symbols were all tuned for a dark background and washed out on a light one
- pointing at a bar in the Summary chart shows the same framed box the other charts show, which turns over at the edge of the window instead of being cut off. The old label always grew upwards from the bar, so on the topmost bar it crossed the top of the chart and half of it was lost
- charts no longer slide left and right when the numbers beside them change size. Room is kept for a figure up to a million, so switching a healing chart between Hull and Shield — an order of magnitude apart — leaves the graph where it was
- the smoothing slider now stops where smoothing still tells you something about the fight: a sixteenth of its length, never less than the 6 seconds it offered before and never more than 60. A long TFO can therefore be smoothed across whole phases from the slider, while on a one-minute patrol a setting that would flatten the whole line into its own edges is out of reach
- the Hull and Shield switches above the healing charts are buttons that stay lit while on, matching the chart and tab switches around them, instead of check boxes

### Fixes
- the DPS, HPS and per-second hit charts now draw at their true height. The smoothing setting used to pull the whole line down as well as smooth it — a little at the default, several percent at the widest the slider offers, and by more than half at the widest value the box accepts — so the chart disagreed with the DPS in the table
- a player keeps one colour and one place in the legend across every chart of a tab. The bar charts sorted them by size while the line charts left them in whatever order the analysis happened to produce, so the same person could be blue on one chart and red on the next
- the two damage charts in Summary no longer call their value "DPS" when it is a damage total. They say Damage Dealt and Damage Taken
- the healing charts follow the Hull and Shield switches when a row is picked in the table. A chart drawn for a selection always started with both halves added together, whatever the switches said, until something else was touched
- taking a series off a per-second chart lets it shrink back. The chart kept room for the line that was removed
- expanding a row now orders its parts on the chart by their own size. Every part carried the total of the row above it instead, so their order was arbitrary

## v1.8.1

### Major Changes
- every combat can carry a short note of your own, up to 50 characters, written in the Summary tab under the combat's name. The note then follows the combat everywhere it is listed — the combats dropdown in the main window, the pick list in Compare Combats and the legend above the comparison table — so four runs of the same map on the same evening stop looking alike. The search box in Compare finds combats by their note too. Notes are remembered between sessions. A note stays with the fight it was written for unless you change how long a pause splits one combat from the next, which re-cuts the log into different combats

### Fixes
- the combats list in the main window is wider, so a combat's name, date and note fit on one line without the entry being cut short
- the list in Compare Combats now starts with the newest combat, like the dropdown in the main window does. It used to start with the oldest, so the runs just played sat at the very bottom of the whole log
- Compare Combats now shows the runs you have just played. The list was the one read when the program last refreshed, and while comparing there is no Refresh button to be found, so combats finished in the meantime were missing. It is now re-read when the view opens and when you go back to picking combats. Combats added this way also arrive with their map, environment and level, so they are no longer dropped by an active filter

### Other Changes
- the healing tabs now start with Self Healing, followed by Healing Ally and Healing Received
- the totals of a split value are now printed in a genuinely bold typeface, heading included. They were only a slightly brighter shade before, which next to the Hull and Shield numbers was easy to miss
- Self Healing no longer offers a "Group by" choice. There is nobody else involved in a heal you did to yourself, so both settings listed the same abilities in the same order, and the only thing the switch could move was the odd heal that came through a pet or a hologram. The tab now always lists abilities, with that pet or hologram underneath the one it carried
- scroll bars no longer cover the table when the pointer comes near them. They used to grow over the content, so the left-right bar hid the bottom row of the table above it; now they have a lane of their own just outside it

## v1.8.0

### Major Changes
- the Compare view can break down each DPS difference into where it came from: the share that came from firing more often and the share that came from each hit landing harder. The two always add up to the whole difference, so a build that trades rate for hit size — or the other way round — stops looking like it changed nothing. Switched on in the Columns menu, alongside the other columns. Hovering either share shows both and their sum, because when the two point opposite ways each on its own can be far larger than the difference they add up to
- healing is now split into three tabs that no longer overlap: Healing Ally (what you healed on others), Healing Received (what others healed on you) and Self Healing (what you healed on yourself, including your own trait and gear procs). Anything you healed on yourself used to be counted in both of the old healing tabs at once, which on a typical run meant a single shield proc accounted for almost all of the healing shown in both — hiding what your team actually did for you and what you did for them

### Fixes
- the combats dropdown shows about fifteen entries again instead of three. Combat names grew when the map's environment and level were added to them, so they wrapped onto two or three lines each inside a list that was too narrow, and only a few of those blocks fitted
- comparing combats now opens on the same player in each of them. Each combat used to be opened on its own highest-DPS player, and in a team that is rarely the same person, so the differences compared one player against another instead of one player's runs — which read as random. Changing the player on the reference combat moves the others to the same player too. If a combat did not include that player it keeps its own top one, and a warning above the table says the numbers are from different people
- charts now cover the whole fight. Every line used to start at its own first event and stop at its last, so a healing chart began only at the first heal and several lines on one chart did not line up; bars were also offset by a fraction of a slice and preceded by a run of empty ones
- attacks on an enemy's shields are no longer counted as healing. Some abilities — `Chain Conduit Capacitor` and plain beam arrays among them — write their shield hit in a form the game also uses for shield repairs, so they showed up as if you had been healing the enemies you were shooting. On a typical run over half of what the healing tab listed as done to others was really damage
- the Resistance % column and chart now show what they claim to: the target's hull resistance. They used to mix in the damage dealt to the target's shields, which is stopped by a different stat entirely (shield hardness), so the figure came out far too favourable — on a typical run it read -36% where the real value was around -56%. Shield drains are left out as well, since those are resisted by yet another stat
- the Upload button now always says what happened: it used to do nothing at all, without any message, when the combat could not be read back from the log file, and an upload that produced no ladder entries showed an empty window
- uploads are now written to the log file (when logging is enabled in the settings), including the reason the server gave for rejecting one

### Other Changes
- hovering a damage or healing value no longer pops up a box repeating its hull and shield halves while those have columns of their own — the box covered the numbers next to it. Turn the columns off and the hover box comes back, since it is then the only way to see the split
- the healing tabs group correctly when a heal came through a pet or a console: picking "Ability first" used to put the pet on top instead, so a team mate healed by your hangar pet appeared under the pet's name rather than the ability's
- Self Healing no longer repeats your own name as a level of the tree — there is nobody else involved, so the abilities sit directly under you, and the switch above the table offers "Source" (the console, trait or proc it came from) instead of "Person"
- the healing charts have Hull and Shield switches above them, both on by default, so healing can be looked at as one total or split into what restored hull and what restored shields
- the "Healing Done" tab is now called "Healing Ally", which says what it holds without having to read the tooltip
- the outgoing damage table has a Drain column: damage that strips shields directly rather than through an attack, which no stat that applies to the rest mitigates
- the Compare table is easier to read across: a rule separates each metric's group of columns, the combat numbers are written "#1 (ref)", "#2" and so on to match the legend above, and every header explains what its column holds. A line above the table says what the small coloured number beside each value is
- the Compare view now filters exactly like the main window: the same three pickers for type, level and map, narrowing each other the same way, with the search box above them. The level is a dropdown there too rather than a row of buttons, and it offers Normal and Unknown as well. The map filter uses the combat's own name instead of picking it back out of the displayed label, so a naming rule whose name contains brackets is no longer cut in half
- the total of a split metric is now bold, so it stands out from the Hull and Shield columns beside it
- the hull and shield halves of a value now have their own columns instead of only showing when you hover: damage, hits, healing and heal ticks all show how much went into hull and how much into shields, for every ability. It can be turned off again under Settings → General
- the healing tabs can now be grouped either way round, with a switch above the table: by person first (who healed you, then with what) or by ability first (what was used, then on whom)
- the damage tabs are now called "Damage Dealt" and "Damage Taken" instead of "Outgoing" and "Incoming", and the summary uses the same wording
- new installs now split combats on a 60 second gap instead of 90, matching the value the OSCR server uses when it reads an uploaded log (existing settings are left alone)
- ground Task Force Operations now show their difficulty too, including Normal — previously only space maps did
- rules can now be duplicated with the 🗐 button next to the bin, in both the rule lists and their conditions — the copy appears right below the original and is selected, ready to be renamed
- the list of auto-detected maps grows with the window as well, sharing the height with the naming rules above it: each may take up to half, and whichever needs less gives the rest to the other
- the rule tables in the Analysis settings now grow with the window: making it taller shows more rules instead of leaving empty space below them
- the Analysis settings are now split into sub-tabs (Combat Names, Source Reversal, Custom Grouping, Damage Exclusion) instead of four sections stacked one under another, so each rule table gets the window's full height and shows about twice as many rows
- combats now show whether the map is a space or a ground map, in parentheses after the name (e.g. "[TFO] Into the Hive (Ground)")
- the Task Force Operations "Into the Hive", "Undine Assault" and "Undine Infiltration" are now recognized automatically
- combats in which nobody dealt any damage are no longer listed — standing around on a social map writes fall damage and self-buffs, and each burst of those used to show up as its own empty combat
- the Task Force Operations "Pahvo Dissension", "Khitomer in Stasis", "Battle of Korfez", "Peril Over Pahvo", "Brotherhood of the Sword", "Breach", "Iuppiter Iratus" and "Devil's Heart" are now recognized automatically
- "Peril Over Pahvo" is no longer reported as the patrol "Rescue and Search" — both send the same Mokai ships, so each is now recognized by its own mission objects instead, the last one always shown as Elite since that is the only level it offers
- "Battle at the Binary Stars" now shows its level (Normal, the only one it offers), and its name is spelled the way the wiki does

## v1.7.0

### Major Changes
- the Advanced / Elite level is now worked out from how tough the enemy ships actually were, using one measure that applies to every space map — so the level shows up on far more maps, including ones whose numbers were never collected individually. Normal is recognized as well, though that part is still experimental

### Other Changes
- more maps are recognized automatically: the patrol "The Ninth Rule", the Task Force Operations "Tzenkethi Front" and "Defense of Starbase One", and all five Red Alert events (Borg, Tholian, Tzenkethi, Elachi, Na'kuhl)
- "The Ninth Rule" is recognized whichever enemy faction it rolls, even when a single run mixes two of them
- Red Alerts and "Defense of Starbase One" always show as Normal, since that is the only level they offer
- the overlay now remembers whether it was open, and reopens itself on the next start

### Fixes
- Red Alert: Tholian is no longer reported as Azure Nebula Rescue, and the other Red Alerts no longer mix with the regular maps that field the same enemy ships
- maps recognized by a friendly or mission ship are no longer missed when that ship happens to take no damage during the fight — such combats used to show up as plain "Combat"
- applying settings is much faster when nothing about the analysis changed: the combat log is no longer re-read from scratch, which used to stall the program after moving the overlay
- the ⚠ mark that flags a naming rule overlapping an auto-detected map now also appears when the rule's name carries an extra annotation instead of exactly matching the map name

## v1.6.1
### Fixes
- on Linux the overlay works again outside of a Wayland session: under X11 the Overlay button used to do nothing at all, and now opens the overlay window as it does on Windows (over a full-screen game it still depends on your window manager — a Wayland session remains the reliable one)
- the remembered window size no longer shrinks a little on every start when the UI scale is set to anything other than 100%
- resizing the main window while the Settings window is open no longer makes the log be analyzed again for no reason

### Other Changes
- the window size is remembered in a new place in the settings, so the first start after this update opens the window at its default size once and remembers it again from then on

## v1.6.0

### Major Changes
- added a Compare Combats view: several combats (across all logs in a folder) can be selected and compared side by side, with each part of the damage breakdown lined up group by group and shown as green/red +/- deltas against the first combat; the compared columns are configurable and remembered, and any ability branch can be charted across the compared combats
- combats now show their map and difficulty automatically, worked out from what actually happened in the fight rather than the name: a combat with no matching name rule shows the detected map — tagged [TFO] or [Patrol] — with its Advanced / Elite level in brackets (e.g. "[TFO] Hive Onslaught [Elite]") instead of just "Combat", and the level is added on top of combats a name rule already names. A wide set of Task Force Operations and patrols is recognized, including ones whose enemy faction is randomized and fights that split into pieces. Name rules still decide the base name (they no longer need to add the level themselves), and the settings list the auto-detected maps and flag any name rule that overlaps one

### Other Changes
- the Compare Combats difficulty filter (Advanced / Elite) and the difficulty shown on each combat now come from what actually happened in the fight rather than the combat's name
- a detection rules file placed next to the settings can refresh or tweak how maps and difficulties are recognized, without waiting for a new build

### Fixes
- the Settings window can now be made as tall as wanted (it was capped before), stays within the screen even when a section is expanded, and remembers its size between sessions
- fixed the average non-critical hit damage showing a wrong value on some runs (abilities that scored criticals on shields were miscounted)

## v1.5.3
### Other Changes
- the overlay now shares the main window's graphics device instead of creating a second one, so it uses a little less memory

### Fixes
- opening "Clear Log File" no longer jumps the main view to the newest combat — the combat you were looking at stays open while the delete list refreshes

## v1.5.2
### Other Changes
- opening "Clear Log File" now refreshes the combats list automatically, so it always shows every combat currently in the log

### Fixes
- the combats list now shows the latest combat after pressing "Refresh Now" or reloading a log, even while the overlay is running (previously it could stay stuck on an earlier combat)

## v1.5.1
### Major Changes
- the "Clear Log File" button now opens a list of every combat with checkboxes, so you can pick exactly which combats to delete (with select all / none, and everything but the newest selected by default)

### Other Changes
- the combats list now shows about 15 combats at once and scrolls when there are more
- the combats list loads automatically when the app starts, without pressing "Refresh Now"
- the overlay shows the latest combat immediately when you open it
- on Linux, the live view keeps updating during play even when reading the merged combat log

### Fixes
- with auto refresh on, the view no longer refreshes when nothing changed, so an expanded damage breakdown stays open while you browse
- the oldest combat in a log can now be saved and deleted (previously it could not be read back)

## v1.5.0
### Major Changes
- the overlay now stays on top of the game on Linux, even during full-screen play
- on Linux, combat logs that get split into many files are now automatically merged into a single combat log, so all of your combats show up together (originals are only removed after the merged log is verified byte-for-byte, so nothing is lost)
- decreased the decimal count of most numbers to reduce used space (can be changed to be like before in the settings)

### Other Changes
- the overlay's move and column-picker controls now live on the overlay itself
- the overlay remembers where you left it and matches the look of the main window
- the main window now remembers its size and whether it was maximized, resizes more smoothly, and has a larger minimum size
- the "Browse" button now opens in the folder you last picked a combat log from

### Fixes
- fixed auto refresh stopping to work when changing the logs path

## v1.4.0
### Major Changes
- added "total crit damage", "total non-crit damage", "average crit hit" and "average non-crit hit" damage metrics
- added new diagrams:
  - Hits per Second
  - Hits count
  - Heals per Second
  - Heals count
  
### Other Changes
- added TFO detection rules to default settings
  - Belly of the Beast
  - Kobayashi Maru
  - Devil's Heart
  - Royal Flush
  - Guillotine

### Fixes
- fixed "Pull Together, Fall Apart" not showing up (and any other powers, that show up the same way in the log)
- fixed a rare bug where a log entry was dropped (only observed on Linux)
- fixed CLAs debug log level not being respected

## v1.3.0
### Major Changes
- the ability to upload runs to the OSCR-Server and display them
- the overlay can now be used independent of the auto refresh setting

### Other Changes
- the auto refresh setting can now be set down to 0.1s via the settings slider (this was possible previously but only by editing the value directly)

### Fixes
- fixed that Overlay would not update if the main window was minimized
- fixed damage resistance summary copy not being labeled correctly

## v1.2.1
### Fixes
- fixed Overlay size being incorrect when the monitor scale is any value other than 100% (windows setting)

## v1.2.0
### Major Changes
- added an overlay to the tool
  - can be configured what numbers are displayed
  - can be freely moved with the mouse or docked in the current location (mouse clicks will go through the window)
- added damage out exclusion rules to the settings
  - this can be useful for ignoring things that may cause the combat time to be extended such as warp core explosions from pets
  - or it can be used to remove things from the log that wish to not be included in your DPS numbers
- in the details analysis tabs, you can now freely select multiple rows to be displayed in the diagrams, by holding the CTRL key while selecting table rows

### Fixes
- max one hit tooltips on the damage out table shows the damage source instead of the target
- combat duration % in the summary table show the actual % instead of the duration in time
- fixed damage resistance chart displaying no negative numbers on the y axis
- fixed crash when toggling auto refresh without having a valid log file path

### Other Changes
- file dialogs are now modal
- added the ability to enable/disable auto refresh without opening the settings window
- added more TFO combat names to the default settings
  - Battle of Wolf 359
  - Vault: Ensnared
- added the ability to drag on drop log files into the parser
- added the ability to clear the log without opening the settings window 

### Internal Changes
- updated eframe + egui

## v1.1.0
### Major Changes
- added the kill counts to damage tables
  - by hovering on the a kills table cell a detailed list is displayed, that shows what was killed or what caused the death
- the summary table now also contains separate player and NPC kill counts
- the damage out table can now be expanded one level more to display, to what target damage was dealt
- added more combat names to the default settings
  - Best Served Cold
  - Battle of the Binary Stars
  - Red Alert: Tholian
  - Battle of Korfez

### Other Changes
- added the ability to move rules up and down in the analysis tab of the settings
- the tool version is now displayed in the window title
- fixed a bug that caused the last bar of a bar chart part to be dropped entirely
- a line a DPS graph now has at least the length of 1 second so that it is clearly visible

### Internal Changes
- updated eframe + egui
- added value and name managers to reduce memory usage slightly

## v1.0.0
### Major Changes
- added incoming and outgoing healing tabs with some diagrams

### Other Changes
- fixed a bug that prevented table columns from shrinking in order to fit its content
- added more TFO names to the default settings
- added more clarifications on what the numbers for certain columns mean
- the rows in the summary table are now also selectable for better visualization
- fixed a bug that caused the damage numbers in the diagrams to be doubled when something is selected in the table
- added hits per second numbers
- added hit percentage numbers
- added misses and accuracy numbers
- added damage type infos
- improved status indication
  - it now shows if there was an error when trying to parse the log
  - when hover over the status icon the log file size is displayed
- added the ability to detect additional infos of a combat such as its difficulty (ISE detection has been added as well)
- changed the terminology for "Sub-Source" to "Indirect Source"
- added a little helper window to display all occurred names of a combat to simplify the creation of combat name rules
- removed plasma storm and distributed targeting again from the default settings as these are by far not the only ones with this hull and shield damage split issue
- fixed an issue where for hull ticks, the number for all ticks was displayed
- added an icon to the app
- various small improvements here and there
- disabled log creation in the default settings

### Major Changes
- added the ability to copy a combat summary to the clipboard
- added the ability to save combats
- added damage resistance metrics and a chart

## v0.3.0
### Major Changes
- added the ability to copy a combat summary to the clipboard
- added the ability to save combats
- added damage resistance metrics and a chart

### Other Changes
- added the ability to select the entire row in the damage tables instead of only the entry name
- fixed a bug that would cause the DPS graph to have incorrect spikes at the beginning of a line
- added base damage and DPS metrics
- fixed not being able to parse logs with non player characters (e.g. Boffs)
- tweaked light dark theme
- fixed a crash when entering 0 into the time slice text edit of a chart
- added distributed targeting and plasma storm to default sub source reversal rules
  - these are really weird since shield and hull damage are reported entirely separately in the log, in order to combat their weirdness they were added to the default settings
- added some more TFO names to the default settings
- renamed the settings file to STO_CombatLogAnalyzer_Settings.json to make it more clear that this file belongs to the parser
- integrated the default settings into the exe for people who copy around only the exe, so that they do not loose the default TFO name detection
- some small tweaks to UI here and there

### Internal Changes
- updated eframe + egui
- switched all tables to custom table and removed egui_extras
  - this fixes any sizing bugs that were present in egui_extras
  - and this allows now for supporting the selection of table rows
- cargo update

## v0.2.1 (29.01.2023)
### Fixes
- fixed some abilities (e.g. concentrate firepower) not being counted as outgoing damage

## v0.2.0 (28.01.2023)
### Major changes
- added a new Theme (Light Dark)
- added Summary tab
- added DPS, Damage and Summary diagrams

### Other Changes
- do not count direct self damage (e.g. from fly her apart) as output damage
- added average shield and hull hit metrics via tooltip
- restrict minimum window size

### Fixes
- fixed some incoming damage sources (e.g. CF or some DOTs) not being counted


## v0.1.0 (05.01.2023)
First work in progress release.

Contains basic damage in and out tables.
