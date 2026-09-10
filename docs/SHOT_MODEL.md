# The shot model

What one shot looks like in the combat log: which field names whom, which way
the damage travels, how a single shot is spread over more than one line, and how
a heal is told from an attack.

`docs/COMBATLOG_FORMAT.md` is the sibling document and owns the **numbers** —
what the two magnitude fields mean per line kind, the resistance formulas, and
the mitigation channels. This one owns the **identities and the grouping**: who
fired, who was hit, and which lines belong together. Neither repeats the other.

Every claim below that carries a figure was measured on the maintainer's real
`combatlog.log`: 134 MB, 674 117 parsed lines, 414 044 distinct shots, several
weeks of play across space and ground content. Where a figure comes from a
different log it says so.

## 1. The fields of a line, and who each one is

```
timestamp :: owner_display, owner_internal,
             source_display, source_internal,
             target_display, target_internal,
             event_display, event_internal,
             type, flags, magnitude, base_magnitude
```

| # | field | name in `Record` | who or what it is |
|---|-------|------------------|-------------------|
| 1–2 | owner | `source` | **Who gets the credit.** For anything a player is responsible for, this is the player, even when something else physically fired it. |
| 3–4 | source | `indirect_source` | **What carried it out** — a pet, a console, an anomaly, a deployed object. Empty when the owner acted directly. |
| 5–6 | target | `target` | **Who received it** — the entity that took the damage or the healing. |
| 7–8 | event | `value_name` | The ability, weapon or proc. Field 8 is an internal id (`Pn.…`), constant per ability. |
| 9 | type | `value_type` | `Shield`, `HitPoints`, or a damage type (`Phaser`, `Kinetic`, …). |
| 10 | flags | `value_flags` | `Critical`, `Kill`, `Flank`, `Miss`, `Immune`, `DoT`, `ShieldBreak`. |
| 11–12 | magnitudes | `value` | See `docs/COMBATLOG_FORMAT.md`. |

The naming inside the code is inherited from upstream and is **not** the
reference's naming: what the code calls `source` is the reference's *owner*, and
what the code calls `indirect_source` is the reference's *source*. The field
order is correct; only the two words are swapped. Renaming them would touch
every rule a user has ever written, because `MatchAspect` spells them out.

### 1.1 What an empty field looks like, and what the star means

An identity is a **pair** of fields: a display name and an internal id. When
there is no entity to name, the pair is written in one of two ways, and they are
not interchangeable.

| written as | source pair (3–4) | target pair (5–6) | owner pair (1–2) | meaning |
|---|---:|---:|---:|---|
| `,` + `*` | 611 535 | 15 825 | **0** | the placeholder: there is deliberately nobody here |
| `,` + `,` | 3 309 | — | 2 991 | the fields are simply blank |

The `*` appears only where an *entity* could have stood and did not — never in
the owner pair, never in the event or value fields. Read it as the game writing
"none" rather than leaving a gap. That the owner pair never carries it is the
point: an owner is not supposed to be absent, so when it is, the line is blank
rather than marked as deliberately empty.

Of the 3 309 blank source pairs, 2 965 sit on lines whose owner pair is blank as
well; the remaining 344 have a named owner and are the ones §7.2 examines.

`Entity::parse` (`src/analyzer/parser.rs`) resolves both spellings to
`Entity::None`, so a `Record` carries `source_field_blank` alongside it to keep
them apart. They do not mean the same thing: `*` says the owner acted directly,
while blank turns out to mark a carrier the game stopped naming (§4.6).

### 1.2 Direction is fields 1→5, and it is the only direction there is

A line always reads *owner acted on target*. There is no field that says
"incoming" or "outgoing"; the same line is outgoing for the owner and incoming
for the target, and `Analyzer::process_next_record` files it under both when
both are players. Two consequences worth knowing:

- A line where owner and target are the same player is **self-directed**
  (`Record::is_self_directed` when the target field is empty as well,
  `Record::is_direct_self_damage` for the damage case).
- A line can be filed twice into the *same* player. This is correct for a heal
  (dealt and received are separate pools) and wrong for damage; see §6.

### 1.3 Field 3 is not "a pet"

The field says what carried the effect out, and the game routes a player's own
procs through whatever entity happened to carry them — including entities that
belong to the enemy. Measured examples from the log, all with the player as
owner:

| what stands in field 3 | lines | what it is |
|---|---:|---|
| `Critter_Stationmod_Kvort_T6_Alpha/Beta` | 19 445 | the player's hangar pets |
| `Space_Fed_Shuttle_Type7_Cheyenne_Pet_1` | 9 216 | the player's shuttle pets |
| `Space_Borg_Cruiser_Control` | 34 | an **enemy** ship the player's chained proc passed through |
| `Plasma_Torpedo_Highyield_R3_Borg` | 9 | an **enemy torpedo** in flight |

The log never labels an entity as "mine". What it does state on every line is
whether the owner acted **directly** or **through something**, and that is the
only split the parser can make without guessing.

**The one test that does separate them** — measured, with no overlap at all — is
the pair of roles an entity plays across the whole fight:

| entity | as field 3 | as target of the owner's *damage* | as target of their *heal* |
|---|---:|---:|---:|
| `Critter_Stationmod_Kvort_T6_Beta` | 10 060 | **0** | 164 |
| `Space_Fed_Shuttle_Type7_Cheyenne_Pet_1` | 9 216 | 0 | 0 |
| `Space_Borg_Cruiser_Control` | 34 | **18 603** | 0 |
| `Space_Borg_Cruiser_Raidisode_No_Loot` | 18 | 50 553 | 0 |

A pet is never shot at by its owner but is often healed by them; an enemy that
carried a proc is shot at thousands of times and never healed. Direction alone
is not enough — a player heals their own pet, so damage-versus-heal has to be
part of the test. Nothing in the program uses this yet; it is recorded here
because it is the only measured way to tell the two apart.

## 2. A shot is not a line

One shot writes **one or two lines**, and occasionally more. The two lines of a
shot describe the same event from two sides: what the shields absorbed, and what
reached the hull.

```
one shot at a shielded target
  ├── shield line   type=Shield      magnitude = −(damage to shields)
  └── hull line     type=<element>   magnitude = +(damage to hull)
```

| situation | lines written |
|---|---|
| target has a shield facing up | shield line **and** hull line |
| no facing, or the attack ignores shields | hull line only |
| shields absorbed the whole shot | shield line only |
| a drain is logged | shield line only |

### 2.1 What identifies a shot

`LineKey` (`src/analyzer/parser.rs`) is the four fields two lines of one shot
share: **timestamp, owner id, target id, and the event's display name** — field
7, not the internal id in field 8, which `LineFields::of` steps over. It
deliberately leaves the source field out, because the two halves of one shot can
disagree about it — §4 is entirely about that, and it turned out to be
systematic rather than a curiosity.

Keying on the display name rather than the internal id is a weakness worth
naming: the same ability reads differently on a localised client (§4.6), so two
halves of one shot would still match each other, but nothing else about the key
would carry across clients. Changing it to field 8 is a one-line move and has
not been made only because no evidence yet says it matters.

The timestamp has a resolution of one tenth of a second, so the key is not
unique: two shots of the same weapon at the same target inside the same tenth
share it. Rules built on this key therefore never treat what it gathers as one
event: they read it **in order**, pairing each shield line with the next damage
line still unclaimed (§4.1).

### 2.2 The two lines are not adjacent

Measured over 157 783 shots that wrote both lines:

| distance from the shield line to its hull line | shots | |
|---|---:|---:|
| the very next line | 77 370 | 49.1% |
| 2 lines apart | 43 660 | 27.7% |
| 3 lines apart | 20 210 | 12.8% |
| 4 lines apart | 9 164 | 5.8% |
| 5 lines apart | 4 084 | 2.6% |
| 6 or more | 3 161 | 2.0% |

In half of all shots another shot's lines stand between the two halves. Pairing
by "the next line" would therefore be wrong half the time; pairing has to search
the whole timestamp for a line matching the key.

### 2.3 The order is fixed: shields first, then hull

Taken as a rule with no exceptions. Counted on shots the key collects exactly
one shield line and one hull line for:

| order within a shot | shots | |
|---|---:|---:|
| shield line first, hull line after | 123 627 | **99.991%** |
| hull line first, shield line after | 11 | 0.009% |

The eleven exceptions do not survive inspection. Some are a `Miss` line — type
field empty, both values zero — miscounted as a hull line:

```
26:09:07:14:22:45.2::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Mo'Kai Bird-of-Prey,C[166 Space_Klingon_Raider_Dsc_Mokai],Phaser Beam Array - Fire at Will III,Pn.B7b7ys1,,Miss,0,0
26:09:07:14:22:45.2::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Mo'Kai Bird-of-Prey,C[166 Space_Klingon_Raider_Dsc_Mokai],Phaser Beam Array - Fire at Will III,Pn.B7b7ys1,Shield,,-1932.19,-2690.7
```

The rest are two different shots at one target whose halves were matched across
each other. The game does not always write a miss, so a hull line with no shield
line before it is simply **the next shot**, one whose shield line the game did
not record — not a shot in reverse order.

So: a shield line is followed by its hull line, and anything else is the next
shot. Every rule in this document is built on that and none of them looks
backwards.

### 2.4 A shot can straddle two timestamps

Rare but real: **13 shots** in the log write their shield line in one tenth of a
second and their hull line in the next. Both halves are alone in their own
timestamp, and the ability id, owner and target match across the boundary:

```
26:09:02:17:31:05.0::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Assimilator,C[421 Space_Borg_Battleship_Raidisode],Phaser Beam Array - Fire at Will III,Pn.B7b7ys1,Shield,,-13161.6,-23824.1
26:09:02:17:31:05.1::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Assimilator,C[421 Space_Borg_Battleship_Raidisode],Phaser Beam Array - Fire at Will III,Pn.B7b7ys1,Phaser,Critical,4559.04,15625.5
```

The timestamp is part of `LineKey`, so the parser treats these as two unrelated
records: the shield line finds no companion and is read as a heal if its base
magnitude is zero, and it inherits no source. Widening the key to a window of
two tenths would catch them and would also merge far more genuinely different
shots than it rescues — 13 shots against 123 627 correctly paired is not a
trade worth making.

## 3. Telling a heal from an attack

Three of the four line kinds carry a negative magnitude, so the sign does not
separate them. The type field and the base magnitude do most of the work:

| line | test | outcome |
|---|---|---|
| `HitPoints`, magnitude < 0 | on the line alone | hull heal |
| `HitPoints`, magnitude > 0 | on the line alone | hull damage |
| `Shield`, magnitude > 0 | on the line alone | shield drain (damage) |
| `Shield`, magnitude < 0, base ≠ 0 | on the line alone | shield damage |
| `Shield`, magnitude < 0, base = 0, `ShieldBreak` | on the line alone | shield damage — a facing cannot be broken by healing |
| `Shield`, magnitude < 0, base = 0, no flag | **needs the rest of the shot** | see below |

`RecordValue::new` (`src/analyzer/parser.rs`) applies the table.

The last row is the one case the numbers cannot settle: a shield heal and an
attack fully absorbed by a shield are written identically. `Parser::
look_ahead_over_shot` settles it by reading forward over the timestamp for a
damage line matching the shot's `LineKey`. Found means attack; not found means
heal. A negative `HitPoints` line does **not** count as that damage line —
abilities that restore hull and shields together write one beside the shield
line, and it is a second heal, not the shot's damage half.

**Observable in the program:** these records appear under Healing rather than
Damage Dealt, and a fight's heal totals shift when the rule changes. On the
maintainer's log the rule reclassifies 31 lines (15 049 points of shield
magnitude); on the 212 MB reference log used when the rule was written it
reclassified 2 348 records and added 0.09 s to a 3.3 s parse.

## 4. Who fired: the source a shield line does not name

The game writes the carrier into field 3 of a shot's **hull** line and, for some
carriers, leaves it empty on the **shield** line of the same shot. Read line by
line, one shot is then split between two owners: its hull half is credited to
the pet and its shield half to the player.

```
26:09:09:22:28:… Kestrel,P[…],,*,                    Borg Cube,C[610 …],Quad Cannons,Pn.6zx6ys1,Shield,,-31036.7,-60424.7
26:09:09:22:28:… Kestrel,P[…],Bird-of-Prey,C[643 …], Borg Cube,C[610 …],Quad Cannons,Pn.6zx6ys1,Disruptor,,2808.47,16654.8
                             ^^^^^^^^^^^^ named here and nowhere else
```

Same instant, same target, same ability id — one shot.

### 4.1 The rule

**The k-th shield line of a shot belongs to the k-th damage line of that shot; a
shield line naming no source takes the source of its partner.** Stated without
reference to any particular entity, weapon or class, because nothing in the log
marks those.

It follows from §2.3. A weapon firing several times at one target inside the
same tenth of a second writes `shield₁ hull₁ shield₂ hull₂ …`, and a *group* of
shots is sometimes written as all the shield lines followed by all the hull
lines — in both layouts, counting from the start pairs each shield line with its
own. Every shield line claims a partner, **including one that names a source of
its own**, and a claimed line is marked so the next shield line looks past it
(`PeekedLine::claimed`).

That last part is what makes the owner's own shot safe when a pet is firing the
same weapon at the same target in the same tenth of a second. The pets' shield
lines claim the pets' hull lines, so the owner's shield line — last of the group
— meets the owner's hull line, which names nobody, and stays the owner's.
Verified on the reference log: across **897** shots where both sides name
sources and can therefore be compared, the k-th shield line and the k-th hull
line agree **every time, with no counterexample**. Held by
`the_owners_own_shot_keeps_its_place_among_a_pets`, which fails if only
sourceless lines claim partners.

`Parser::look_ahead_over_shot` does the search and `ShotLookahead::
take_source_of` copies the partner's source across; `Parser::
is_sourceless_shield_line` decides which lines are asked about at all. A partner
that names nobody leaves the shield line naming nobody too — the two halves then
agree that the owner fired it.

The first version of this rule demanded that *every* damage line of the shot name
the same source and gave up otherwise. That threw away every burst two pets
fired into, and read a chained effect arriving from three entities as one
unanswerable shot rather than as three shots.

### 4.2 What the rule does and does not cover

Counted by running the real parser over the whole log — every shield line whose
own text names no source:

| | lines | shield magnitude |
|---|---:|---:|
| **inherited a source from its shot** | **10 053** | **133 762 069** |
| left naming nobody | 190 124 | — |

The 190 110 are overwhelmingly lines that should name nobody: 139 976 belong to
shots whose damage lines name nobody either — the owner really did fire them —
and about 50 000 are heals, which have no damage line at all and never did.
What is genuinely lost is small: 176 shield lines are damage fully absorbed by a
facing with no damage line anywhere in the shot, so nothing exists to pair with.

A shot the key gathers several damage lines for — a chained effect reaching one
target through a Probe and a Sphere inside the same tenth of a second — is read
as what it is: several shots, in the order written. The first takes the shield
line; the others are shots whose own shield line the game did not record.

**Observable in the program:** a pet's row on the Damage Dealt tab carries a
non-zero Shield figure, and a weapon the player does not carry no longer appears
as a row of their own damage. Before the rule, one fight showed `Quad Disruptor
Cannons` as 93 hits of the player's own damage for a weapon that only their
hangar pets mounted.

### 4.3 Which carriers hide their source

Not all of them do, and the split is per entity class rather than per weapon or
per damage type:

| carrier | hull lines | shield lines |
|---|---:|---:|
| `Space_Fed_Shuttle_Type7_Cheyenne_Pet_1` | 5 376 | 3 834 |
| `Space_Intel_Emp_Probe_Projectile_2` | 2 923 | 1 889 |
| `Critter_Stationmod_Kvort_T6_Alpha` | 9 098 | **0** |
| `Critter_Stationmod_Kvort_T6_Beta` | 9 712 | **0** |

The control that rules out weapon and target: against the *same* Borg entity in
the *same* fight, the player writes shield lines on 43.8% of shots and their
hangar pet on 0.0% of 646. Why one class of entity behaves differently is
unknown; the rule above does not depend on knowing, because it asks the shot
rather than the entity.

### 4.4 Two other reasons a shield line has no partner

Separate from the above, and both about the **target** rather than the shooter:

- **Objects have no shields.** Enemy torpedoes, mines and platforms take hull
  damage only: `Plasma_Torpedo_Highyield_R2_Borg` (208 lines), `Elachi_Torpedo_
  Highyield_*`, `Mine_Electric_High_Repel_Drantzuli_Alpha` (2 127) — 0% shield
  lines, against 25–42% for Borg ships. A name is not the test:
  `Mission_Infected_Healer_Probe_Scaling` reads as a probe and is a ship at
  36.4%.
- **Kinetic and physical damage mostly bypasses facings.** Of the owner's own
  shots: `Kinetic` writes a shield line on 7.9% and `Physical` on 0.5%, against
  36–43% for every energy type.

Neither is a defect to correct. They are why a torpedo boat's shield figures are
near zero and an energy build's are not.

### 4.5 The decision, end to end

Every line goes through the same questions. Nothing in this path knows the name
of a weapon, a pet or an ability — only the shape of the fields and what the
fight has already shown.

```
  a line of the log
        │
        ├── is field 9 "Shield"?  ── no ──┐
        │                                 │
       yes                                │
        │                                 │
        ▼                                 │
  read ahead over the shot                │
  (key: timestamp, owner, target,         │
   event name; §2.1)                      │
        │                                 │
        ├── a damage line of this shot,   │
        │   not yet claimed?              │
        │        │                        │
        │       yes ── mark it claimed,   │
        │              take two things    │
        │              from it:           │
        │              · its source, if   │
        │                this line names  │
        │                none             │
        │              · whether its own  │
        │                source pair was  │
        │                left blank       │
        │        │                        │
        │        no ── nothing to learn   │
        │                                 │
        └────────────────┬────────────────┘
                         ▼
        what does the source pair say now?
                         │
     ┌───────────────────┼────────────────────┐
     ▼                   ▼                    ▼
  a name           the "*" placeholder     blank  ",,,"
     │                   │                      │
     ▼                   ▼                      ▼
  that carrier      the owner fired it    ask the fight about
  fired it          themselves            this event id (§4.6):
     │                   │                      │
     │                   │        ┌─────────────┼──────────────┐
     │                   │        ▼             ▼              ▼
     │                   │   one carrier   several        the owner also
     │                   │   ever, never   carriers,      fires this id
     │                   │   the owner     never owner    here, or nothing
     │                   │        │             │         seen yet
     │                   │        ▼             ▼              │
     │                   │   that carrier  (Damage owner       ▼
     │                   │                  unknown)      the owner
     └───────────────────┴────────┴─────────────┴──────────────┘
                                  ▼
                   the shot is laid into the tree under
                   whoever it ended up belonging to
```

Separately, and about the **owner** rather than the carrier: a line whose owner
pair is blank (2 991 in the reference log) is credited to nobody, and the row it
lands in on the target's Damage Taken is named after the event, because that is
the only thing such a line says about where the damage came from (§6.7).

Reading it as prose: **the owner is never inferred** — field 1–2 is taken as
written, and the only question ever asked is what carried the shot out. That
question is asked in two stages: first the shot itself (does its damage line
name a carrier?), then, only for the blank spelling, the fight (has this event
id ever been fired directly here?).

Three properties follow, and they are what makes the rule survive new content:
### 4.6 What the internal event id settles, and what it does not

The internal id (field 8, `Pn.…`) does not say who fired a line. What it does
give is a way to ask the *rest of the fight* about a line that names nobody.

**Its meaning is stable.** Measured across 100 ladder logs from other players,
2021 to 2026: 1 691 distinct ids, of which 1 034 appear in more than one log and
632 in more than one year. `Pn.W86lgg1` is the same ability in all six years.
Where an id appears under two different names it is the client's **language**,
not a change of meaning — `Pn.Ca3ukc1` is `Gravity Well I` and
`Gravitationsquelle I`, `Pn.Cedjls` is `Phaser Array` and `Phaserstrahlenbank`.
Ignoring localisation, 69.6% of the ids spanning several years keep the exact
same name.

That is worth knowing for **grouping rules**: a rule written against a display
name does not survive a German client, while one written against `Pn.` does.

**Entity ids are the opposite.** `C[…]` numbers an instance for the session, not
a thing: of 2 365 distinct numbers in those logs, 1 325 (56%) stand for more
than one entity — `C[232]` is 22 different entities across the set. Nothing in
this program keys on them, and that is deliberate; grouping identifies a pet by
its **display name**, so a row means the same thing in every fight.

**What the id can answer.** Ask it only within a single fight, and only of hull
lines, which never hide the shooter:

> If, in this fight, every hull line carrying this id names a carrier and none
> has the `*` placeholder, then a line of that id whose source pair is blank
> was fired by a carrier, not by the owner.

Measured on the maintainer's log: of 364 lines with a blank source pair, **303
(83%)** are settled that way — for 125 of them only one carrier ever used the id,
so even *which* carrier is known. 45 have no hull line to judge by and 15 use an
id fired both ways.

Those 15 are a real class, not noise: `Kemocite-Laced Weaponry I`,
`Antiproton Retort` (all four facings), `Mycelial Lightning`,
`Hellbore Light - Ignition`. All are **effects the player applies and something
else detonates**, so the id genuinely belongs to both. The rule declines them by
construction — the `*` placeholder is present, so the test fails — and they stay
with the player, which is where an applied effect belongs.

Across a single fight the id is unambiguous in **96%** of cases (100 ladder
logs: 66.5% owner-only, 29.9% carrier-only, 3.2% both).

**This is implemented.** `Combat::update_carrier_evidence` accumulates the
evidence as records arrive — from hull lines only, since a shield line hides its
carrier — and `Combat::who_fired` answers it for a line whose source pair is
blank. The answer travels into `Player::build_grouping_path`, which lays the
record out as a carried shot.

Nothing is deferred and nothing is moved after the fact. Asking the evidence
*as it stands when the line arrives* costs almost nothing against asking it
after the whole fight: measured on the reference log, 301 of 364 lines are
settled in flight against 303 settled with hindsight — a difference of **two
lines**. That keeps the parser a stream, and keeps an incremental refresh
identical to a cold read, since the evidence depends only on what came earlier
in the same fight.

Three outcomes, and the third is the one worth arguing about:

| the fight's hull lines for this id say | the line goes to |
|---|---|
| only ever one carrier, never the owner | that carrier |
| several carriers, never the owner | a row named `(Damage owner unknown)` |
| the owner fires it too, or nothing seen yet | the owner, as before |

The middle row is a **new row in the damage tree**, shown as
**`(Damage owner unknown)`** — that exact wording is used in the program, the
changelog and this document, so the case has one name everywhere. Note it does
not mean the line's *owner* field (§1) is missing: that is always there. It
means the program cannot say which of the owner's carriers fired it.

Merging those lines into the player's own weapon row would put damage under a
weapon they may not carry; dropping them would lose real damage; picking one of
the carriers would be a guess wearing an exact figure. Naming the gap is the
only option that states what is actually known. On the reference fight it holds
13 hits and 206 471 damage, against the 1 557 hits the two named pets hold.

**Both halves of a shot go together.** A shield line carries the `*`
placeholder even when its shot was left unsigned, so judging it on its own would
leave it with the player while its hull line went to the carrier — one shot
split across two rows. `ShotLookahead::partner_source_blank` carries the
partner's unsigned status back to the shield line, so the whole shot is judged
once. On one real fight that alone moved 7 further hits, doubling the row.

### 4.7 Validated against a hundred other players' logs

The rules above were derived from one log. To check them against data they had
never seen, 100 combat logs were fetched from the OSCR ladder
(`/ladder-entries/` for the index, `/combatlog/{id}/download/` for each log,
identifying `User-Agent`, 0.5 s between requests): 260 MB of text, 2021–2026,
different players, builds and maps. The fetch took 118 s; listing the index took
302 s and is the slow part.

**Method — hide the answer and see if it comes back.** Every shield line that
*names* a carrier had that carrier replaced with the `*` placeholder, so it
looked exactly like the lines the rule has to solve. The real parser then read
the altered logs, and its answers were compared against what had been hidden.
176 130 lines:

| | lines | |
|---|---:|---:|
| recovered the right carrier | 96 508 | 54.79% |
| left naming nobody — declined to guess | 78 839 | 44.76% |
| **named the wrong carrier** | **783** | **0.445%** |

So the rule is wrong about one line in 225, and in 45% of cases says nothing
rather than inventing an answer.

The errors concentrate: three logs supply most of them (12.4%, 9.7% and 6.4%
wrong), and all three are builds flying **several different pets that fire into
one target at once** — `Obelisk Swarmer` beside `Advanced Valkyrie Fighter` and
others. That is the shape §4.1's ordering assumption is weakest against, and it
is the shape to test first when this is revisited.

**A trap worth recording.** The first attempt at this validation compared the
sequence of sources on a shot's shield lines against the sequence on its hull
lines, and reported a 2% mismatch. It was measuring nothing: a shot key gathers
lines that are *not* pairs, so the comparison put a pet's shield line beside an
unrelated hull line —

```
SHIELD src=Obelisk Swarmer   Shield      -200.424   -222.693
SHIELD src=Obelisk Swarmer   Shield       -189.74   -210.822
hull   src=Borg Cube         AntiProton   272.181    494.874
hull   src=Borg Cube         AntiProton   257.672    468.494
```

— and called the difference a failure of a rule that never touches lines which
already name a source. Only masking a known answer and asking the real parser to
recover it measures anything.

### 4.8 What time cannot tell us

The rule of §4.6 declines wherever several carriers used one event id in a
fight, and sends those lines to `(Damage owner unknown)`. The obvious way to
fill that gap is with recency: a pet that fired this weapon at this target a
moment ago probably fired it again. Measured against ground truth — hull lines
whose carrier is known, hidden and re-derived — over the 100 ladder logs, and
**restricted to the case the current rule declines**:

| asked | answers | accuracy | wrong |
|---|---:|---:|---:|
| the last carrier of this weapon at this target | 90.5% | 75.4% | 11 558 |
| …only if exactly one carrier fired in the last 0.5 s | 49.4% | 86.9% | 3 350 |
| …last 1 s | 53.6% | 91.9% | 2 248 |
| …last 2 s | 53.4% | 93.6% | 1 758 |
| …last 5 s | 51.4% | 94.1% | 1 584 |

So recency does carry *some* signal, and the stricter question — "was exactly
one carrier shooting here recently?" rather than "who shot last?" — is worth
15 points of accuracy. It is still not enough. At its best it answers half the
open cases and is wrong about one in sixteen of them, and those wrong answers
land in pet rows that are currently exact.

The reason is visible in the data: two Bird-of-Prey pets fire into one target
several times a second, so "who fired last" is close to a coin toss, and even a
window with a single carrier in it is often an accident of timing. **51 813** of
the 278 434 carrier-named hull lines in those logs sit on an event id that more
than one carrier used in the same fight, so this is the common shape, not an
edge case.

Left unimplemented deliberately. It is recorded here with its numbers so the
idea is not re-derived from scratch, and so that if the row ever grows large
enough to be a nuisance, the trade is already priced: about half of it
recoverable, at roughly a 6% error rate.

## 5. Where this departs from the published references

There is no specification from Cryptic. Two sources are treated as prior art,
and this document disagrees with both in specific places.

**The community reference** is the r/stobuilds wiki page `math/log_reading`
(compiled by Mastajdog from a 2015 thread). Reddit and its mirrors refuse
automated requests; fetch the archived copy:

```
curl --compressed \
  "http://web.archive.org/web/20250806033015/https://www.reddit.com/r/stobuilds/wiki/math/log_reading/"
```

**The reference implementation** is [OSCR](https://github.com/STOCD/OSCR), the
parser behind the ladder — `OSCR/parser.py` in particular.

| topic | what the reference says | what we do | evidence |
|---|---|---|---|
| field 3 | "Display name of source (only appears if Pet/Gravity Well etc)" — the wiki | treated as *whatever carried this out*, which includes enemy entities and objects in flight | §1.2 |
| differing sources on one shot | the wiki notes it as a caveat: pairing "cannot key on that field" | same conclusion, and then the caveat is used as a signal: a shield line takes the source of the next unclaimed damage line of its shot | §4 |
| shield line, magnitude < 0, base ≥ 0 | OSCR: `is_heal` on the line alone, no look-ahead | look ahead over the shot; a matching damage line makes it an attack | §3 |
| attributing a pet's damage | OSCR: `if line.source_name:` per line — a shield line with an empty field 3 is credited to the owner | the shot decides, not the line | §4.1 |
| damage with no owner | OSCR: filed under an empty identity | the effect's own name stands in for the shooter | §6 |
| pet identity | OSCR splits a pet group by name and then a row per instance id | one row per pet **name**; instances of the same name merge | `Player::build_grouping_path` |
| grouping rules | not applicable — OSCR has no user rules | a user's grouping rule folds the ability level only, never the carrier level | `docs/ARCHITECTURE.md` §"What each rule may move" |

The first four are deliberate departures with measurements behind them. The
fifth is a known difference, not a decision: merging instances loses the ability
to see which of three identical hangar pets did the work.

## 6. What is still wrong

1. **A line can count twice against one player.** Where the owner is a player,
   field 3 names an enemy entity and the target is that same player, the record
   is filed into both `damage_out` (the source is a player) and `damage_in` (the
   target is a player). 12 lines in the log — `Mycelial Lightning` 9,
   `Refracting Tetryon Cascade` 2, `Kemocite` 1. Nobody has decided whether such
   a line is outgoing damage that came back or incoming damage under the
   player's name.
2. **176 shield lines cannot be attributed at all.** Damage fully absorbed by a
   facing, no damage line to pair with, no source named — 0.09% of the shield
   lines in the log. They stay with the owner.
3. **13 shots split across two timestamps** (§2.4) and are read as two
   unrelated records. The order of the two lines is treated as fixed (§2.3), so
   nothing looks backwards for a partner.
3a. **Where several carriers share an event id, the line is not attributed at
   all** — it goes to `(Damage owner unknown)`. Recency was measured as a way to
   fill this in and rejected at a 6% error rate; see §4.8 for the numbers.
4. **The pairing key is not unique** at a tenth-second resolution. The rules
   built on it require agreement rather than picking a candidate, so a collision
   costs an attribution rather than producing a wrong one.
5. **About 13% of blank-source lines arrive before the fight can judge them.**
   The evidence in §4.6 is what the fight has seen *so far*, so a line landing
   before its carrier has fired a hull line stays with the owner. Measured: 47
   of 364. Judging after the whole fight would recover two of them, at the cost
   of holding records back — see §4.6 for why that trade was refused.
6. **One hull line carries negative damage** and is counted as a negative
   contribution to a player's total (§7.2). One line in 674 117; left alone
   because no rule that would catch it can be stated without also catching
   legitimate heals.
7. **2 991 lines name no owner at all.** They are filed as incoming damage on
   their target and outgoing for nobody, so a fight's total damage dealt does not
   equal its total damage taken. That is a property of the log, not a defect to
   fix, but anything comparing the two totals has to know it. The row they land
   in is named after the effect (`Player::add_in_value`), since that is the only
   thing such a line says about where the damage came from; before that it was
   drawn as `<unknown>`.

## 7. A catalogue of line shapes

The log holds **115 distinct shapes** if a shape is taken to be the combination
of *(what stands in the owner pair, the source pair, the target pair; the type;
the sign of each magnitude)*. Below is one **unmodified** line per family,
copied out of the log, with what each field is doing in it. Together they cover
every shape with more than a few hundred occurrences, plus the oddities that
matter for attribution.

### 7.1 The ordinary cases

**The owner fires their own weapon at an enemy.** Two lines, one shot.

```
26:09:01:11:38:34.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Sphere,C[421 Space_Borg_Cruiser_Raidisode_Sibrian_Elite_Initial],Terran Task Force Phaser Beam Array - Fire at Will III,Pn.8rvb8h,Shield,,-8917.7,-10226.5
26:09:01:11:38:34.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Sphere,C[421 Space_Borg_Cruiser_Raidisode_Sibrian_Elite_Initial],Terran Task Force Phaser Beam Array - Fire at Will III,Pn.8rvb8h,Phaser,Critical,1956.96,10587.2
```

Fields 1–8 are byte-identical across the pair; only type, flags and the two
numbers differ. **Who fired is beyond doubt**: the owner is a player, the source
pair is the `*` placeholder on both halves, so nothing carried it. 336 976
lines have this shape on the hull side and 117 862 on the shield side — it is
the bulk of any log.

**A pet fires.** The source pair names the carrier:

```
26:09:01:11:38:32.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],Anti-Time Entanglement Singularity (Rank 2),C[426 Temporal_Holding_Tier5_Click_V2],Sphere,C[422 Space_Borg_Cruiser_Raidisode_Sibrian_Elite_Initial],Anti-Time Entanglement Singularity (Rank 2),Pn.L942mu1,Physical,Critical,3120.26,2693.72
```

36 426 lines. Note that fields 3 and 7 carry the same words here: the carrier is
named after the ability that spawned it. That is why grouping keys on the
interned handle rather than the string — see `docs/ARCHITECTURE.md`.

**An enemy fires at the player.** The same shape with the roles swapped:

```
26:09:01:11:38:30.2::Borg Cube,C[416 Space_Borg_Dreadnought_Raidisode_Sibrian_Initial_Boss],,*,Raman,P[2123318@4450574 Raman@ramanwaleczny],Heavy Plasma Cannon,Pn.Ejahfy,Plasma,ShieldBreak,1962.28,20054.6
```

**An enemy's own pet fires at the player** — 3 925 lines. Field 3 belongs to the
*owner in field 1*, never to the target:

```
26:09:01:11:41:28.7::Borg Cube,C[613 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Nanite Sphere,C[620 Space_Borg_Cruiser_Raidisode_Heal_Cube_No_Loot],Raman,P[2123318@4450574 Raman@ramanwaleczny],Plasma Array,Pn.4g1l7r,Plasma,,79.2908,4301.34
```

**Healing the owner's own ship, from their own gear** — 1 200 lines. The target
is the owner, the source is their console, and the base magnitude is zero:

```
26:09:01:11:42:21.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],Bio-Molecular Shield Generator (Rank 2),C[672 Undine_Holding_Tier_5_Shield_Generator_V2],Raman,P[2123318@4450574 Raman@ramanwaleczny],Bio-Molecular Shield Generator Fabrication (Rank 2),Pn.Lfgxse1,Shield,,-881.25,0
```

**Healing with no target at all** — 11 057 shield lines and 3 073 hull lines.
Both identity pairs after the owner are the `*` placeholder; the target is the
owner by implication:

```
26:09:01:11:38:31.5::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,,*,Reflexive Emitters,Pn.D5jwvs,Shield,,-2000,0
26:09:01:11:38:43.1::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,,*,Brace for Impact III,Pn.X88tg61,HitPoints,,-180,-150
```

**A miss** — 2 851 lines. The type field is *empty*, both numbers are zero, and
the flag carries the whole meaning:

```
26:09:01:11:38:35.5::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Heavy Plasma Torpedo,C[430 Plasma_Torpedo_Highyield_R2_Borg],Phaser Beam Array - Fire at Will III,Pn.B7b7ys1,,Miss,0,0
```

**A shield drain** — a `Shield` line with a *positive* magnitude and zero base;
22 932 lines against the player, 9 216 by the player. Not damage in the ordinary
sense: it resolves against DrainX rather than shield hardness.

```
26:09:01:11:38:31.4::Sphere,C[421 Space_Borg_Cruiser_Raidisode_Sibrian_Elite_Initial],,*,Raman,P[2123318@4450574 Raman@ramanwaleczny],Tachyon Beam III,Pn.Jzk8g9,Shield,,1854.4,0
```

### 7.2 The unclear cases

Each of these is a shape where "who fired this" cannot be answered from the line
alone, and in most of them not at all.

**A shot whose shield half names nobody and whose hull half names a pet.** This
is the case §4 is about, and the reason the two lines cannot be read
independently:

```
26:09:09:22:28:20.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Borg Cube,C[610 Space_Borg_Dreadnought_Raidisode_Sibrian_Initial_Boss],Quad Disruptor Cannons - Rapid Fire III,Pn.6zx6ys1,Shield,,-31036.7,-60424.7
26:09:09:22:28:20.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],Bird-of-Prey (BETA),C[643 Critter_Stationmod_Kvort_T6_Beta],Borg Cube,C[610 Space_Borg_Dreadnought_Raidisode_Sibrian_Initial_Boss],Quad Disruptor Cannons - Rapid Fire III,Pn.6zx6ys1,Disruptor,,2808.47,16654.8
```

Resolved: the shield line takes `Bird-of-Prey (BETA)`.

**The owner and a pet firing the same weapon at the same target, in the same
tenth of a second.** 36 shots in the log, 15 of them owned by a player. The game
writes the group as all its shield lines and then all its hull lines, in the
same order of shooters on both sides:

```
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],Nanite Sphere,C[267 …Heal_Cube_No_Loot],Raman,P[…],Plasma Array,Pn.4g1l7r,Shield,,…
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],Nanite Sphere,C[239 …Heal_Cube_No_Loot],Raman,P[…],Plasma Array,Pn.4g1l7r,Shield,,…
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],,*,Raman,P[…],Plasma Array,Pn.4g1l7r,Shield,ShieldBreak,-3433.…
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],Nanite Sphere,C[267 …Heal_Cube_No_Loot],Raman,P[…],Plasma Array,Pn.4g1l7r,Plasma,…
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],Nanite Sphere,C[239 …Heal_Cube_No_Loot],Raman,P[…],Plasma Array,Pn.4g1l7r,Plasma,…
26:09:02:11:58:39.3::Borg Cube,C[214 …Final_Boss],,*,Raman,P[…],Plasma Array,Pn.4g1l7r,Plasma,ShieldBreak,1790.4…
```

Resolved, and only because every shield line claims a partner: the third shield
line is the Borg Cube's own and meets the third hull line, which is also its
own. Had only the sourceless shield line looked for a partner it would have
taken the first hull line and credited the cube's shot to one of its spheres.
The same shape is what a player would produce by mounting the weapon their
hangar pets carry — rare in this log, ordinary in a build that does it.

**Both source fields blank rather than the placeholder** — 344 lines, 308 of
them owned by a player. The abilities that show this shape are overwhelmingly
ones that normally *do* have a carrier:

```
26:09:01:11:38:40.7::Raman,P[2123318@4450574 Raman@ramanwaleczny],,,Borg Cube,C[416 Space_Borg_Dreadnought_Raidisode_Sibrian_Initial_Boss],Anti-Time Entanglement Singularity (Rank 2),Pn.L942mu1,Physical,Critical,4955.4,2443.23
```

`Anti-Time Entanglement Singularity` is carried 824 times and fires directly
**zero** times, so this line certainly had a carrier the game did not name. But
the shape is not a reliable marker on its own: 178 of the 308 belong to
abilities used both ways, and `Kemocite-Laced Weaponry I` appears 8 973 times as
a genuinely direct proc and 13 times in this shape. **Unresolved** — these lines
are still credited to the owner.

**The owner fields themselves are blank** — 2 991 lines. Something hit the
target and the log does not say what:

```
26:09:01:11:39:58.2::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Nanite Transformer,C[412 Mission_Borgraid1_Comm_Array],Phaser Beam Array - Fire at Will I,Pn.J148es1,Phaser,Critical,17568.9,14017.9
26:09:01:11:39:58.3::,,,,Raman,P[2123318@4450574 Raman@ramanwaleczny],Plasma Torpedo,Pn.Rm7fzt,Plasma,DoT,226.406,954.319
26:09:01:11:39:58.3::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Nanite Transformer,C[412 Mission_Borgraid1_Comm_Array],Phaser Beam Array - Fire at Will I,Pn.J148es1,Phaser,Critical,18070,14417.8
```

The middle line reads: 226.4 plasma damage over time landed on Raman, from
nobody. 793 of these carry a damage type and hit a player. `Entity::parse`
returns `Entity::None` for the owner, so `Analyzer::process_next_record` files
the record into the target's incoming damage and into nobody's outgoing —
which is the only honest reading available.

**A player standing in the *source* field of an enemy's line** — 392 lines,
almost all `Plasma Fire`, the Borg damage-over-time left burning on a ship:

```
26:09:01:11:41:22.5::Borg Cube,C[613 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Plasma Fire,Pn.Wujkxq,Plasma,DoT,1755.82,6519.86
```

Owner is the Borg Cube, the carrier is *the player*, and there is no target at
all. `Analyzer::process_next_record` has a dedicated arm for exactly this: when
the source pair is a player and the owner is not, the record is filed as
incoming damage on that player.

**A bridge officer as the carrier** — 146 lines, identity type `S[…]` rather
than `C[…]`, with no unique name inside the brackets:

```
26:09:02:12:48:50.7::Raman,P[2123318@4450574 Raman@ramanwaleczny],SRO SCI 2,S[155587856],Attendant,C[481 Ground_Drantzuli_Alpha_Ensign_Summoned],Hyperonic Radiation I,Pn.Taqn8j1,Radiation,DoT,12.5622,138.184
```

`Entity::NonPlayerCharacter` covers this. Ground content only; the away team is
carried the same way a pet is.

**Negative damage** — one single line in 674 117, and it has no explanation:

```
26:09:09:20:44:06.0::Raman,P[2123318@4450574 Raman@ramanwaleczny],,*,Probe,C[216 Space_Borg_Frigate_Mirror],Immolating Phaser Lance,Pn.I2fr5t,Phaser,Critical,-628.115,-354.296
```

A hull line whose magnitude is negative would be a heal by every rule in §3, yet
its type is `Phaser` and it is flagged `Critical`. `RecordValue::new` reaches
its final branch and records it as hull damage of −628, which subtracts from the
player's total. Seven more lines are the same idea from the other direction:
an enemy's `Plasma Torpedo` with magnitude `-7.62939e-06`, floating-point noise
around zero.

**The four lines that started all of this** — a map on which the player's ship
carried no such weapon at all, yet the log signs them with nobody:

```
26:09:09:22:31:40.3::Raman,P[2123318@4450574 Raman@ramanwaleczny],,,Borg Cube,C[853 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Quad Disruptor Cannons,Pn.S1g0uw1,Disruptor,,9376.27,5661.44
26:09:09:22:31:40.4::Raman,P[2123318@4450574 Raman@ramanwaleczny],,,Borg Cube,C[853 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Quad Disruptor Cannons,Pn.S1g0uw1,Disruptor,Critical,29387,17744
26:09:09:22:31:40.6::Raman,P[2123318@4450574 Raman@ramanwaleczny],,,Borg Cube,C[853 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Quad Disruptor Cannons,Pn.S1g0uw1,Disruptor,Critical,30491.9,18411.1
26:09:09:22:31:40.8::Raman,P[2123318@4450574 Raman@ramanwaleczny],,,Borg Cube,C[853 Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss],Quad Disruptor Cannons,Pn.S1g0uw1,Disruptor,,10358.6,6254.57
```

All four carry the blank-blank source shape, so nothing *in them* says a pet
fired — and nothing says the player's ship did either. What settles them is the
rest of the fight: `Pn.S1g0uw1` has 3 985 hull lines naming a Bird-of-Prey and
not one carrying the `*` placeholder, so the owner never fired it here (§4.6).
Both Bird-of-Prey pets used that id, so which of them is beyond the log, and the
four lines land in **`(Damage owner unknown)`** — 4 hits, 79 614 damage, beside
the 9 hits of `Disruptor Turret` that reach it the same way.

The player's own row for that weapon is gone from this fight entirely, which is
correct: they were not carrying it.

## Related

- `docs/COMBATLOG_FORMAT.md` — what the two magnitude fields mean, the
  resistance formulas, and what the log cannot tell you at all.
- `docs/ARCHITECTURE.md` — where a parsed record lands in the metric trees, and
  which level of that tree each kind of user rule may move.
- `docs/HEALING_MODEL.md` — the three healing pools and how a heal is routed
  into one of them.
