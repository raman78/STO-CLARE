//! Several runs per build instead of one each: what survives when the same
//! build is flown more than once.
//!
//! `build_diff` explains the difference between *two* runs. That is the wrong
//! unit for the question the reader actually has, because two runs cannot tell a
//! build difference from a run that went well. This module groups runs by the
//! name the reader gave them and asks what holds across every pairing.
//!
//! Three things become answerable here that a single pair cannot answer, and
//! each of them is cheap:
//!
//! - **A noise floor.** The spread of one build's own runs is what any
//!   difference between builds has to beat. It is measured, not assumed.
//! - **Direction agreement.** A factor that moves the same way in every pairing
//!   is established; one that changes sign between pairings is noise wearing a
//!   number. This needs no distributional assumption at all.
//! - **An exact rank test.** With a handful of runs the orderings can simply be
//!   counted, so the report can state the true smallest attainable result rather
//!   than quote a p-value from a table that assumes more data than exists.
//!
//! What it deliberately does **not** do is average the runs together and diff
//! the averages. An average of three runs hides whether the three agreed, and
//! whether they agreed is the entire question.

// Drawn by nothing yet; see `build_diff`.
#![allow(dead_code)]

use rustc_hash::{FxHashMap, FxHashSet};

use super::build_diff::{Factors, Run};

/// The middle, the ends, and how many there were.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stats {
    pub n: usize,
    pub median: f64,
    pub low: f64,
    pub high: f64,
}

impl Stats {
    pub fn of(values: &[f64]) -> Option<Stats> {
        if values.is_empty() {
            return None;
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(f64::total_cmp);
        let middle = sorted.len() / 2;
        let median = if sorted.len().is_multiple_of(2) {
            (sorted[middle - 1] + sorted[middle]) / 2.0
        } else {
            sorted[middle]
        };
        Some(Stats {
            n: sorted.len(),
            median,
            low: sorted[0],
            high: sorted[sorted.len() - 1],
        })
    }

    /// Largest less smallest — the noise floor when the values are runs of one
    /// build.
    pub fn spread(&self) -> f64 {
        self.high - self.low
    }
}

/// Every run the reader gave one name, and the map they were flown on.
#[derive(Clone, Debug)]
pub struct Group {
    pub label: String,
    pub runs: Vec<Run>,
    /// What each run's map and difficulty were detected as. Held so the report
    /// can say when a comparison is mixing content rather than quietly letting
    /// a harder map read as a worse build.
    pub maps: Vec<String>,
}

impl Group {
    /// Damage per second, hull and shields together — the figure the tables
    /// show. The verdict is on this and not on hull potential: see
    /// `Tally::total_dps`.
    pub fn potentials(&self) -> Vec<f64> {
        self.runs
            .iter()
            .map(|run| run.whole.total_dps(run.duration_s))
            .collect()
    }

    /// Per run, the four factors over the whole of it. `None` for a run that
    /// dealt no hull damage, which is dropped rather than counted as zero — a
    /// factor of zero is not a measurement of anything.
    pub fn factors(&self) -> Vec<Factors> {
        self.runs
            .iter()
            .filter_map(|run| run.whole.factors(run.duration_s))
            .collect()
    }

    /// Whether every run was flown on the same map at the same difficulty.
    pub fn one_map(&self) -> Option<&str> {
        let mut unique: FxHashSet<&str> = FxHashSet::default();
        for map in &self.maps {
            unique.insert(map.as_str());
        }
        (unique.len() == 1).then(|| self.maps[0].as_str())
    }
}

/// How consistently a row belongs to one build rather than the other.
///
/// The distinction the reader needs and cannot see: a row in every run of one
/// build and no run of the other **is** the build difference, while a row in two
/// runs out of three is loadout drift between sessions and should not be read as
/// part of it.
#[derive(Clone, Debug)]
pub struct Presence {
    pub name: String,
    pub in_a: usize,
    pub in_b: usize,
    /// Median DPS over the runs that have it, per group.
    pub dps: [Option<f64>; 2],
}

impl Presence {
    /// In all of one group's runs and none of the other's.
    pub fn is_systematic(&self, n_a: usize, n_b: usize) -> bool {
        (self.in_a == n_a && self.in_b == 0) || (self.in_b == n_b && self.in_a == 0)
    }
}

/// Which way a factor moved, counted over every pairing of a run of one group
/// with a run of the other.
#[derive(Clone, Copy, Debug)]
pub struct Agreement {
    pub factor: &'static str,
    /// Pairings where the factor was larger in group B.
    pub up: usize,
    pub down: usize,
    /// The median ratio B/A over the pairings.
    pub median_ratio: f64,
}

impl Agreement {
    pub fn pairings(&self) -> usize {
        self.up + self.down
    }

    /// Every pairing moved the same way. With three runs each that is nine
    /// pairings agreeing, which is worth stating plainly; with one each it is
    /// vacuous, and `pairings` lets the reader see which case they are in.
    pub fn unanimous(&self) -> bool {
        self.pairings() > 1 && (self.up == 0 || self.down == 0)
    }
}

/// The comparison of two named sets of runs.
pub struct GroupVerdict {
    pub groups: [Group; 2],
    pub potential: [Option<Stats>; 2],
    pub agreements: Vec<Agreement>,
    pub presences: Vec<Presence>,
    pub rank: RankTest,
}

/// Counting the orderings, which is the only honest test at this sample size.
#[derive(Clone, Copy, Debug)]
pub struct RankTest {
    pub n_a: usize,
    pub n_b: usize,
    /// Pairings in which the B run beat the A run, ties counting a half.
    pub b_wins: f64,
    /// Which group the pairings favour: `Some(0)` for A, `Some(1)` for B,
    /// `None` when they split evenly.
    ///
    /// Held explicitly because `p` is one-sided and has to be one-sided *in the
    /// direction the runs actually point*. Reporting it always against B gives
    /// `p = 1.0` for a perfect separation in A's favour, which reads as "nothing
    /// here" at the exact moment the evidence is strongest.
    pub favours: Option<usize>,
    /// Exact one-sided probability of a separation at least this clean in the
    /// favoured direction, if the two builds were the same and only the runs
    /// differed.
    pub p: f64,
    /// The smallest `p` these run counts could possibly produce. When this is
    /// not small, no result from this many runs can settle anything, and that
    /// is a fact about the run count rather than about the builds.
    pub best_possible_p: f64,
    /// Every run of the favoured group beat every run of the other, so the two
    /// sets of runs do not overlap at all.
    pub complete: bool,
}

impl GroupVerdict {
    pub fn of(a: Group, b: Group) -> Self {
        let (pa, pb) = (a.potentials(), b.potentials());
        let potential = [Stats::of(&pa), Stats::of(&pb)];
        let rank = rank_test(&pa, &pb);

        let (fa, fb) = (a.factors(), b.factors());
        let agreements = (0..Factors::NAMES.len())
            .map(|i| {
                let mut up = 0usize;
                let mut down = 0usize;
                let mut ratios: Vec<f64> = Vec::new();
                for x in &fa {
                    for y in &fb {
                        let (x, y) = (x.as_array()[i], y.as_array()[i]);
                        if y > x {
                            up += 1;
                        } else if y < x {
                            down += 1;
                        }
                        if x > 0.0 {
                            ratios.push(y / x);
                        }
                    }
                }
                Agreement {
                    factor: Factors::NAMES[i],
                    up,
                    down,
                    median_ratio: Stats::of(&ratios).map(|s| s.median).unwrap_or(1.0),
                }
            })
            .collect();

        let presences = presences_of(&a, &b);

        GroupVerdict {
            groups: [a, b],
            potential,
            agreements,
            presences,
            rank,
        }
    }
}

/// Per row name, in how many runs of each group it appears and what it did.
fn presences_of(a: &Group, b: &Group) -> Vec<Presence> {
    let mut counts: FxHashMap<String, ([usize; 2], [Vec<f64>; 2])> = FxHashMap::default();
    for (side, group) in [a, b].into_iter().enumerate() {
        for run in &group.runs {
            for row in &run.rows {
                let entry = counts.entry(row.name.clone()).or_default();
                entry.0[side] += 1;
                entry.1[side].push(row.tally.total_dps(run.duration_s));
            }
        }
    }

    let mut presences: Vec<Presence> = counts
        .into_iter()
        .map(|(name, (in_both, dps))| Presence {
            name,
            in_a: in_both[0],
            in_b: in_both[1],
            dps: [
                Stats::of(&dps[0]).map(|s| s.median),
                Stats::of(&dps[1]).map(|s| s.median),
            ],
        })
        .collect();
    // Systematic first, then by what is at stake: a row that decides the
    // comparison should not be below one that neither build leans on.
    let weight = |p: &Presence| {
        p.dps[0].unwrap_or(0.0).max(p.dps[1].unwrap_or(0.0))
    };
    let (n_a, n_b) = (a.runs.len(), b.runs.len());
    presences.sort_by(|x, y| {
        y.is_systematic(n_a, n_b)
            .cmp(&x.is_systematic(n_a, n_b))
            .then(weight(y).total_cmp(&weight(x)))
    });
    presences
}

/// The exact one-sided rank test: how many of the ways these values could have
/// been split into two groups of these sizes separate them at least as cleanly
/// as they actually are.
///
/// Enumerated rather than approximated. At three runs a side there are twenty
/// splits, so the true figure is a loop, and a normal approximation at that
/// size would be a made-up number where an exact one is free. Above
/// `MAX_SPLITS` the enumeration is skipped and `p` is left at 1.0, which reads
/// as "not established" rather than as a claim.
fn rank_test(a: &[f64], b: &[f64]) -> RankTest {
    const MAX_SPLITS: usize = 200_000;
    let (n_a, n_b) = (a.len(), b.len());
    // Pairings won by the second of the two slices handed in.
    let wins = |x: &[f64], y: &[f64]| -> f64 {
        let mut wins = 0.0;
        for left in x {
            for right in y {
                wins += match right.total_cmp(left) {
                    std::cmp::Ordering::Greater => 1.0,
                    std::cmp::Ordering::Equal => 0.5,
                    std::cmp::Ordering::Less => 0.0,
                };
            }
        }
        wins
    };
    let b_wins = wins(a, b);
    let pairings = (n_a * n_b) as f64;
    let favours = if b_wins * 2.0 > pairings {
        Some(1)
    } else if b_wins * 2.0 < pairings {
        Some(0)
    } else {
        None
    };
    // The count the test is on: whichever direction the runs point.
    let observed = b_wins.max(pairings - b_wins);
    let none = RankTest {
        n_a,
        n_b,
        b_wins,
        favours,
        p: 1.0,
        best_possible_p: 1.0,
        complete: false,
    };
    if n_a == 0 || n_b == 0 {
        return none;
    }

    let pooled: Vec<f64> = a.iter().chain(b.iter()).copied().collect();
    let total = pooled.len();
    let splits = binomial(total, n_a);
    let complete = observed >= pairings;
    if splits > MAX_SPLITS as f64 {
        return RankTest {
            best_possible_p: 1.0 / splits,
            complete,
            ..none
        };
    }

    let mut at_least_as_clean = 0usize;
    let mut counted = 0usize;
    let mut chosen = vec![0usize; n_a];
    combinations(total, n_a, 0, 0, &mut chosen, &mut |pick| {
        let picked: FxHashSet<usize> = pick.iter().copied().collect();
        let side_a: Vec<f64> = (0..total)
            .filter(|i| picked.contains(i))
            .map(|i| pooled[i])
            .collect();
        let side_b: Vec<f64> = (0..total)
            .filter(|i| !picked.contains(i))
            .map(|i| pooled[i])
            .collect();
        let split_wins = wins(&side_a, &side_b);
        // Counted in both directions, because the hypothesis under test is
        // "these two builds differ this cleanly", not "B is the better one".
        if split_wins.max(pairings - split_wins) >= observed {
            at_least_as_clean += 1;
        }
        counted += 1;
    });

    RankTest {
        n_a,
        n_b,
        b_wins,
        favours,
        p: at_least_as_clean as f64 / counted.max(1) as f64,
        // The cleanest separation is the two splits that put one group wholly
        // above the other, one for each direction.
        best_possible_p: 2.0 / splits,
        complete,
    }
}

fn binomial(n: usize, k: usize) -> f64 {
    let k = k.min(n - k);
    (0..k).map(|i| (n - i) as f64 / (i + 1) as f64).product()
}

fn combinations(
    n: usize,
    k: usize,
    start: usize,
    depth: usize,
    chosen: &mut Vec<usize>,
    visit: &mut impl FnMut(&[usize]),
) {
    if depth == k {
        visit(chosen);
        return;
    }
    for i in start..=n - (k - depth) {
        chosen[depth] = i;
        combinations(n, k, i + 1, depth + 1, chosen, visit);
    }
}

impl GroupVerdict {
    pub fn report(&self) -> String {
        let mut out = String::new();
        let [a, b] = &self.groups;

        out.push_str("Runs, and the content they were flown on\n");
        for group in [a, b] {
            match group.one_map() {
                Some(map) => out.push_str(&format!(
                    "  {:<24} {} runs, all on {}\n",
                    group.label,
                    group.runs.len(),
                    map
                )),
                None => {
                    out.push_str(&format!(
                        "  {:<24} {} runs, on MORE THAN ONE kind of content:\n",
                        group.label,
                        group.runs.len()
                    ));
                    for (run, map) in group.runs.iter().zip(&group.maps) {
                        out.push_str(&format!("      {:<28} {}\n", run.label, map));
                    }
                }
            }
        }
        out.push('\n');

        out.push_str("DPS, the figure the tables show\n");
        for (group, stats) in [a, b].into_iter().zip(self.potential) {
            match stats {
                Some(s) => out.push_str(&format!(
                    "  {:<24} median {:>10.0}   own runs {:.0} to {:.0}  (spread {:.0}, {:.1}%)\n",
                    group.label,
                    s.median,
                    s.low,
                    s.high,
                    s.spread(),
                    if s.median > 0.0 {
                        100.0 * s.spread() / s.median
                    } else {
                        0.0
                    }
                )),
                None => out.push_str(&format!("  {:<24} no runs\n", group.label)),
            }
        }
        if let [Some(sa), Some(sb)] = self.potential {
            let gap = sb.median - sa.median;
            out.push_str(&format!(
                "  {:<24} {:>+17.0}  ({:+.1}%)\n",
                "difference of medians",
                gap,
                if sa.median > 0.0 {
                    100.0 * gap / sa.median
                } else {
                    0.0
                },
            ));
            // The verdict is whether the two sets of runs overlap, not whether
            // the gap beats a range. A range grows with the run count and one
            // good run stretches it, so comparing a difference of medians
            // against it called a clean separation "inside the noise" — with
            // the rank test on the next line saying the opposite. Two tests
            // that disagree in one report is worse than either alone.
            let (better, worse) = if gap >= 0.0 { (&b.label, &a.label) } else { (&a.label, &b.label) };
            out.push_str(&format!(
                "  {:<24} {}\n",
                "reads as",
                if self.rank.complete {
                    format!(
                        "clean: every {better} run beat every {worse} run, with nothing in between"
                    )
                } else {
                    let overlap = sa.low.max(sb.low)..sa.high.min(sb.high);
                    format!(
                        "overlapping: the two builds' runs share the range {:.0} to {:.0}, so \
                         some runs of the worse one beat some runs of the better",
                        overlap.start, overlap.end
                    )
                }
            ));
        }
        out.push('\n');

        let r = self.rank;
        out.push_str(&format!(
            "Counting the orderings: {} of {} pairings went to {}, {} to {}\n",
            r.b_wins,
            r.n_a * r.n_b,
            b.label,
            r.n_a as f64 * r.n_b as f64 - r.b_wins,
            a.label
        ));
        out.push_str(&format!(
            "  ahead: {}\n",
            match r.favours {
                Some(0) => a.label.as_str(),
                Some(1) => b.label.as_str(),
                _ => "neither — the pairings split evenly",
            }
        ));
        out.push_str(&format!(
            "  exact p = {:.3} for a split this clean either way; the best these {} and {} runs \
             could give is {:.3}\n",
            r.p, r.n_a, r.n_b, r.best_possible_p
        ));
        if r.best_possible_p > 0.05 {
            out.push_str(&format!(
                "  at these run counts no result can reach 0.05 however the runs fall; four a \
                 side is the first count that can ({:.3})\n",
                2.0 / binomial(8, 4)
            ));
        }
        out.push('\n');

        out.push_str("Did each factor move the same way every time?\n");
        for agreement in &self.agreements {
            out.push_str(&format!(
                "  {:<30} {} up / {} down of {} pairings, median {:.3}x{}\n",
                agreement.factor,
                agreement.up,
                agreement.down,
                agreement.pairings(),
                agreement.median_ratio,
                if agreement.unanimous() {
                    "   <- every pairing agreed"
                } else {
                    ""
                }
            ));
        }
        out.push('\n');

        let (n_a, n_b) = (a.runs.len(), b.runs.len());
        out.push_str("Rows that belong to one build and not the other\n");
        let mut any = false;
        for presence in &self.presences {
            if !presence.is_systematic(n_a, n_b) {
                continue;
            }
            any = true;
            let (side, count, dps) = if presence.in_a > 0 {
                (&a.label, presence.in_a, presence.dps[0])
            } else {
                (&b.label, presence.in_b, presence.dps[1])
            };
            out.push_str(&format!(
                "  {:>10.0}   {:<46} every one of {side}'s {count} runs, none of the other's\n",
                dps.unwrap_or(0.0),
                presence.name
            ));
        }
        if !any {
            out.push_str("  (none — no row is in all of one build's runs and none of the other's)\n");
        }

        out.push_str("\nRows that come and go between sessions (loadout drift, not the build)\n");
        let mut drift = 0usize;
        for presence in &self.presences {
            if presence.is_systematic(n_a, n_b) {
                continue;
            }
            if presence.in_a == n_a && presence.in_b == n_b {
                continue;
            }
            let at_stake = presence.dps[0].unwrap_or(0.0).max(presence.dps[1].unwrap_or(0.0));
            if at_stake < 1_000.0 {
                continue;
            }
            drift += 1;
            out.push_str(&format!(
                "  {:>10.0}   {:<46} {} of {} {} runs, {} of {} {} runs\n",
                at_stake,
                presence.name,
                presence.in_a,
                n_a,
                a.label,
                presence.in_b,
                n_b,
                b.label
            ));
        }
        if drift == 0 {
            out.push_str("  (none worth 1k or more)\n");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

/// Groups a real log's combats by the note the reader gave them and reports
    /// what holds across every pairing.
    ///
    /// `CLA_TEST_GROUPS` is two groups separated by `;`, each a `|`-separated
    /// list of the notes that belong to it, so two names for one build can be
    /// pooled:
    ///
    /// `CLA_TEST_COMBATLOG=<path> CLA_TEST_GROUPS='APB|Attack Patern Beta;HBL|Hellbore' \
    ///   cargo test build_groups_on_a_real_log -- --ignored --nocapture`
    #[test]
    #[ignore = "reads a real STO log"]
    fn build_groups_on_a_real_log() {
        use crate::analyzer::{Analyzer, settings::AnalysisSettings};
        use crate::app::damage_subset::metrics_duration;
        use crate::app::settings::{CombatNotes, Settings};

        let Some(path) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this");
            return;
        };
        let Ok(spec) = std::env::var("CLA_TEST_GROUPS") else {
            println!("set CLA_TEST_GROUPS, e.g. 'APB|Attack Patern Beta;HBL|Hellbore'");
            return;
        };
        let wanted: Vec<Vec<String>> = spec
            .split(';')
            .map(|group| group.split('|').map(|s| s.trim().to_string()).collect())
            .collect();
        if wanted.len() != 2 {
            println!("CLA_TEST_GROUPS needs exactly two groups separated by ';'");
            return;
        }

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .expect("the log opens");
        analyzer.update();
        let combats = analyzer.result();
        let notes = Settings::load_or_default().combat_notes;

        let group_of = |names: &[String]| {
            let mut group = Group {
                label: names[0].clone(),
                runs: Vec::new(),
                maps: Vec::new(),
            };
            for combat in combats.iter() {
                let note = notes.get(&CombatNotes::key(combat));
                if !names.iter().any(|n| n == note) {
                    continue;
                }
                let Some((_, player)) = combat.players.iter().max_by(|(_, x), (_, y)| {
                    x.damage_out.dps.all.total_cmp(&y.damage_out.dps.all)
                }) else {
                    continue;
                };
                group.runs.push(Run::of(
                    &player.damage_out,
                    &combat.name_manager,
                    &combat.hits_manger,
                    metrics_duration(&player.combat_time),
                    format!(
                        "{note} {}",
                        combat.active_time.start.format("%m-%d %H:%M")
                    ),
                ));
                group.maps.push(combat.name().to_string());
            }
            group
        };

        // Printed beside the diff's own figure because the reader compares
        // builds by the DPS the tables show, and a verdict resting on a
        // different quantity has to be shown to agree with that one — or be
        // the wrong quantity.
        println!("\nPer run, in the analyzer's own figures and in this module's");
        println!(
            "  {:<26} {:>6}  {:>12}  {:>12}  {:>12}  {:>8}",
            "run", "secs", "DPS (all)", "this module", "hull potent.", "gap"
        );
        for names in &wanted {
            for combat in combats.iter() {
                let note = notes.get(&CombatNotes::key(combat));
                if !names.iter().any(|n| n == note) {
                    continue;
                }
                let Some((_, player)) = combat.players.iter().max_by(|(_, x), (_, y)| {
                    x.damage_out.dps.all.total_cmp(&y.damage_out.dps.all)
                }) else {
                    continue;
                };
                let duration = metrics_duration(&player.combat_time);
                let run = Run::of(
                    &player.damage_out,
                    &combat.name_manager,
                    &combat.hits_manger,
                    duration,
                    String::new(),
                );
                // The gap is the check that matters: this module's own sums
                // have to come to the analyzer's DPS, or the verdict is about a
                // quantity nothing else in the program agrees with.
                let mine = run.whole.total_dps(duration);
                println!(
                    "  {:<26} {:>6.0}  {:>12.0}  {:>12.0}  {:>12.0}  {:>7.3}%",
                    format!("{note} {}", combat.active_time.start.format("%m-%d %H:%M")),
                    duration,
                    player.damage_out.dps.all,
                    mine,
                    run.whole.hull_potential_dps(duration),
                    100.0 * (mine - player.damage_out.dps.all) / player.damage_out.dps.all,
                );
            }
        }

        let verdict = GroupVerdict::of(group_of(&wanted[0]), group_of(&wanted[1]));
        println!("\n{}", verdict.report());
    }

    /// The test is two-sided, because the reader is asking which of two builds
    /// is better and not confirming a direction picked in advance. That halves
    /// the reachable certainty: of the twenty ways six runs can be split three
    /// and three, **two** separate them completely — one for each direction —
    /// so a perfect result is one in ten and not one in twenty.
    #[test]
    fn a_perfect_separation_of_three_against_three_is_one_in_ten() {
        let test = rank_test(&[100.0, 110.0, 120.0], &[130.0, 140.0, 150.0]);
        assert_eq!(9.0, test.b_wins, "every pairing went to B");
        assert_eq!(Some(1), test.favours);
        assert!(test.complete, "the two sets of runs do not overlap");
        assert!((test.p - 0.1).abs() < 1e-9, "p = {}", test.p);
        assert!((test.best_possible_p - 0.1).abs() < 1e-9);
    }

    /// The same separation the other way round has to read as strongly, and in
    /// the other build's favour. Reporting `p` always against the second group
    /// gave 1.0 here — "nothing to see" for the cleanest evidence available.
    #[test]
    fn a_separation_in_the_first_group_favour_reads_just_as_strongly() {
        let test = rank_test(&[130.0, 140.0, 150.0], &[100.0, 110.0, 120.0]);
        assert_eq!(0.0, test.b_wins, "no pairing went to B");
        assert_eq!(Some(0), test.favours);
        assert!(test.complete);
        assert!((test.p - 0.1).abs() < 1e-9, "p = {}", test.p);
    }

    /// The fact that decides how many runs are worth flying. Four a side is the
    /// first count whose best possible result clears 0.05; three cannot, however
    /// the runs fall.
    #[test]
    fn four_runs_a_side_is_the_first_count_that_can_settle_anything() {
        let best = |n: usize| {
            let a: Vec<f64> = (0..n).map(|i| 100.0 + i as f64).collect();
            let b: Vec<f64> = (0..n).map(|i| 200.0 + i as f64).collect();
            rank_test(&a, &b).best_possible_p
        };
        assert!((best(2) - 2.0 / 6.0).abs() < 1e-9, "{}", best(2));
        assert!((best(3) - 2.0 / 20.0).abs() < 1e-9, "{}", best(3));
        assert!((best(4) - 2.0 / 70.0).abs() < 1e-9, "{}", best(4));
        assert!(best(3) > 0.05, "three a side cannot reach 0.05");
        assert!(best(4) < 0.05, "four a side can");
    }

    #[test]
    fn interleaved_runs_are_not_a_separation() {
        let test = rank_test(&[100.0, 130.0, 150.0], &[110.0, 120.0, 140.0]);
        assert!(test.p > 0.4, "p = {}", test.p);
    }

    /// A row in every run of one build and none of the other is the build; the
    /// same row in two runs out of three is something that changed between
    /// sessions, and the two must not be reported as one thing.
    #[test]
    fn a_row_in_every_run_of_one_build_is_systematic_and_two_of_three_is_not() {
        let systematic = Presence {
            name: "Hellbore".to_string(),
            in_a: 3,
            in_b: 0,
            dps: [Some(30_000.0), None],
        };
        let drifting = Presence {
            name: "Some Array".to_string(),
            in_a: 2,
            in_b: 1,
            dps: [Some(30_000.0), Some(20_000.0)],
        };
        assert!(systematic.is_systematic(3, 3));
        assert!(!drifting.is_systematic(3, 3));
    }

    #[test]
    fn a_factor_that_changes_sign_between_pairings_is_not_unanimous() {
        let agreed = Agreement {
            factor: "targets softer",
            up: 9,
            down: 0,
            median_ratio: 1.07,
        };
        let mixed = Agreement {
            factor: "fired more often",
            up: 5,
            down: 4,
            median_ratio: 1.01,
        };
        assert!(agreed.unanimous());
        assert!(!mixed.unanimous());
    }

    /// One pairing agreeing with itself is not agreement, and must not read as
    /// though it were.
    #[test]
    fn a_single_pairing_is_never_unanimous() {
        let single = Agreement {
            factor: "targets softer",
            up: 1,
            down: 0,
            median_ratio: 1.2,
        };
        assert!(!single.unanimous());
    }

    #[test]
    fn a_spread_is_the_ends_and_the_middle_is_the_median() {
        let stats = Stats::of(&[120.0, 100.0, 110.0]).unwrap();
        assert_eq!(110.0, stats.median);
        assert_eq!(100.0, stats.low);
        assert_eq!(120.0, stats.high);
        assert_eq!(20.0, stats.spread());
        assert_eq!(None, Stats::of(&[]));
    }
}
