# Ladder Upload (OSCR)

How a combat gets from this app onto the OSCR ladder, and every rule that can
stop it. Sources: our `src/upload/upload.rs`, `STOCD/OSCR-server`
(`combatlog/views/combatlog.py`, `combatlog/models/combatlog.py`,
`ladder/models/ladder.py`, `combatlog/serializers/combatlog.py`,
`combatlog/urls/combatlog.py`, `api-spec.yaml`), `STOCD/OSCR`
(`OSCR/iofunc.py`) and `STOCD/OSCR-UI` (`OSCRUI/apiclient.py`), read 2026-07-30
and 2026-08-09.

There is **no health or ping endpoint**. The whole API is `/combatlog/…`,
`/ladder…`, `/variant…` and `/system/latest/` (OSCR's own update feed, which
answers `500` on the live server). A cheap liveness check is therefore
`GET /ladder/`, which the Records window already performs.

## It is manual, one combat at a time

There is no automatic upload. The user selects a combat and presses
**"Upload 🌎"** (`Upload::show`). One press = one combat.

## No separate log file is needed

A common misconception. The app does **not** ask the user to carve out a file
containing a single fight — it does it itself:

```
Combat::read_log_combat_data()   seek to log_pos.start, read to log_pos.end
   -> gzip (flate2, best)
   -> multipart POST  <oscr_url>/combatlog/uploadv2/
      part "file", filename = Combat::name()
      3 s to connect, 60 s for the answer
```

`log_pos` is the byte range the combat occupies in the original log, recorded
while parsing. So the payload is a byte-exact slice of the untouched log — no
rewriting, no re-serialising.

**Gzip is fine.** The server writes the uploaded bytes to a temp file as-is and
hands it to the OSCR parser, which sniffs the first two bytes for the gzip magic
`1f 8b` and transparently decompresses (`OSCR/iofunc.py`).

**The slice must start at the combat**, because the server parses only the
**first** combat it finds:

```python
parser_settings = {"combats_to_parse": 1, "seconds_between_combats": 60, ...}
parser.analyze_log_file(max_combats=1)
combat = parser.combats[0]
```

Note `seconds_between_combats: 60` — the server splits combats on a 60 s gap,
hard-coded, with no setting or environment variable behind it. A slice cut with a
longer separation can therefore contain more than the server will look at, and
only its first part counts. Our own default was 90 s and is now 60 s to match.

## The server re-detects everything

The filename is only a label. Map and difficulty come from the server running
its own detection over the bytes (`combat.map`, `combat.difficulty`). Renaming a
combat locally cannot influence the ladder — and our own map/difficulty detection
has no bearing on it either.

## Which endpoint, and why the status code is not the answer

The server offers two, both live and both anonymous. We use **`uploadv2`**, which
is also the only one the OSCR client itself calls
(`OSCR-UI/OSCRUI/apiclient.py`). `upload` (v1) still answers, but nothing
official exercises it any more.

The difference is error handling. v1 has none: a log the parser cannot read
raises out of the view and reaches the client as an HTTP error with no reason in
it. v2 wraps the same work in `try/except` and answers **`200` either way**:

| answer                                       | means                                      |
|----------------------------------------------|--------------------------------------------|
| `results: [...]`, `combatlog: <id>`, `detail`| read; the rows are the ladder outcome      |
| `results: []`                                | read, but it matched no ladder — a success |
| `results` absent or `null`, `detail: "..."`  | not read; `detail` is the reason           |

So `results` — not the status code — is what tells success from failure. Treating
`200` as success would report an unreadable log as an upload; that is the one
mistake this endpoint makes easy, and `UploadResponseV2 -> UploadOutcome` in
`src/upload/upload.rs` exists to make it once, in a tested place.

`combatlog` is the id of the stored log. It names a page on the ladder site,
`<oscr_url>/ui/combatlog/<id>/` (`combatlog/urls/combatlog.py`), which the upload
window offers as a link once the server has said what it stored.

## Why an upload gets rejected

Raised as an exception, which v2 hands back as `detail`:

| reason                                                            | message                                                            |
|-------------------------------------------------------------------|--------------------------------------------------------------------|
| no parsable combat                                                | `Combat log is empty`                                              |
| no players in it                                                  | `Combat log is empty`                                              |
| map/difficulty not laddered, or outside the variant's date window | `<map> (<difficulty> Difficulty) at <time> has no matching ladder` |

The ladder lookup matches `internal_name` **and** `internal_difficulty` (or a
ladder with `internal_difficulty = None`), and requires
`variant.start_date <= combat.start_time` and `variant.end_date > combat.end_time`.
Variants can also exclude space or ground for a date range. This is why most maps
cannot be uploaded at all: only a small authorised set has ladder rows.

## Why an entry does not update, even though the upload succeeded

These are per-player and do **not** fail the upload — they come back in
`results[]` with `updated: false`:

| reason                        | detail                                                                 |
|-------------------------------|------------------------------------------------------------------------|
| prohibited ability used       | `<ability> is a prohibited ability. Ladder entry will not be updated.` |
| combat time below threshold   | `<player>'s combat time was too low.`                                  |
| player banned                 | `<player> is banned from the ladder.` (`BlockedHandle`)                |
| solo ladder, group run        | silently skipped (`ladder.is_solo and len(players) != 1`)              |
| existing entry already better | `No updates for <player> on <ladder>`                                  |

The combat-time threshold is a fraction of either the top player's combat time,
the log duration, or the player duration, depending on
`variant.combat_time_source`.

Above `ladder.manual_review_threshold` the entry is stored with `visible=False`
and the response says it needs manual review, quoting the combat log ID.

Only the **best** result per player per ladder is kept: an entry is written when
none exists, or updated only when the new value beats the stored one on that
ladder's `metric`.

## Not the same thing as the DPS League

[sto-league.com](https://www.sto-league.com/updated-rules-for-uploading-combatlogs/)
publishes upload rules — no modified, trimmed or incomplete logs, on pain of a
ban — but those govern the **DPS League** and its CombatLogReader (CLR) tool, a
separate leaderboard. They are not what the OSCR server enforces. The OSCR
endpoint takes anonymous uploads (`permission_classes=()`) and enforces only the
mechanical checks above.
