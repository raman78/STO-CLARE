//! Turning a comparison of two runs into an explanation of the difference
//! between them: which discrete sources one run had and the other did not, and
//! how the rest of the difference splits between firing more often, hitting
//! harder, critting harder, and meeting a softer target.
//!
//! The design and the reasoning behind it are in `docs/BUILD_DIFF.md`; this is
//! the arithmetic. Two rules from there govern everything here:
//!
//! - **Effects, never causes.** Nothing in this module names an ability as the
//!   reason for anything. An ability that deals no damage has no row in the log
//!   at all, so a resistance debuff can only ever be seen in what it did to
//!   everyone else's rows.
//! - **The account closes.** Every split sums to the difference it is a split
//!   of, exactly. A column of numbers that does not add up to the figure above
//!   it invites arithmetic that is wrong.
//!
//! Everything is measured on the **hull channel**. Damage dealt to shields
//! carries no base damage in the log (`SpecificHit::Shield` has no
//! `base_damage`), so it cannot enter a decomposition that has target
//! mitigation as one of its factors. What the factors multiply out to is
//! therefore "what the target's hull would have taken per second", not the DPS
//! the tables show.

// Nothing in the UI calls any of this yet: the branch exists to find out what
// the arithmetic can pull out of a real log before any of it is drawn. The
// ignored test `build_diff_on_a_real_log` is the only caller.
#![allow(dead_code)]

use rustc_hash::FxHashMap;

use crate::analyzer::{
    AnalysisGroup, DamageGroup, Hit, HitsManager, NameManager, SpecificHit, ValueFlags,
};

/// The least hull hits a row needs, in **both** runs, before its ratios are
/// read as evidence of anything. A row of a dozen hits has a ratio dominated by
/// which of them happened to crit.
pub const MIN_HITS_FOR_A_RATIO: u64 = 50;

/// How far from the median a row's ratio may sit and still count as agreeing
/// with it, as a fraction.
///
/// Provisional. `docs/BUILD_DIFF.md` "Calibration left open" says why a fixed
/// number is the wrong long-term answer and what replaces it; until there are
/// enough runs to fit one, this is a starting point to be moved by the reader
/// rather than a measured constant.
pub const DEFAULT_TOLERANCE: f64 = 0.05;

/// How many of a cluster's rows must agree with the median before the whole
/// thing is called one effect rather than a coincidence.
pub const DEFAULT_AGREEMENT: f64 = 0.8;

/// The raw sums every figure here is built from, for one row or one whole run.
///
/// Kept as sums rather than as the analyzer's finished metrics because the crit
/// split has to be taken on **base** damage, and `DamageMetrics` splits crits on
/// damage actually dealt. Measured on a real log, crit-flagged hull hits carry
/// about 2.4x the base damage of non-crit hits of the same weapon, so the two
/// splits are not interchangeable — see `docs/BUILD_DIFF.md` stage 1.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Tally {
    /// Hull hits whose base damage was counted — that is, not `IMMUNE`. The
    /// analyzer counts an immune hit in `hits` but adds none of its damage, so
    /// counting it here would break `cadence * neutral = base_dps`.
    pub hull_hits: u64,
    pub crit_hull_hits: u64,
    /// Damage of the shot before the target mitigated any of it.
    pub base_damage: f64,
    pub crit_base_damage: f64,
    /// Damage that reached the hull.
    pub hull_damage: f64,
    /// Damage dealt to shields, drains included — the other half of the figure
    /// the tables call Total Damage. Stripping a shield is work the build has to
    /// do, so leaving it out measured something the reader never reads.
    pub shield_damage: f64,
    /// Damage the shields stopped from reaching the hull. Not damage dealt: the
    /// hull-damage equivalent of what they absorbed, which is the other half of
    /// the resistance numerator.
    pub prevented_to_hull: f64,
}

impl Tally {
    pub fn add_hit(&mut self, hit: &Hit) {
        // Immune contributes nothing anywhere, exactly as the analyzer has it
        // (`DamageMetrics::calc_and_apply_delta`). Counting it would put damage
        // in this module's totals that no table shows.
        if hit.flags.contains(ValueFlags::IMMUNE) {
            return;
        }
        match hit.specific {
            SpecificHit::Hull { base_damage } => {
                self.hull_hits += 1;
                self.base_damage += base_damage;
                self.hull_damage += hit.damage;
                if hit.flags.contains(ValueFlags::CRITICAL) {
                    self.crit_hull_hits += 1;
                    self.crit_base_damage += base_damage;
                }
            }
            SpecificHit::Shield {
                damage_prevented_to_hull,
            } => {
                self.shield_damage += hit.damage;
                self.prevented_to_hull += damage_prevented_to_hull;
            }
            // A drain deals shield damage and carries neither a hull component
            // nor a base damage, so it counts towards the total and towards no
            // factor.
            SpecificHit::ShieldDrain => self.shield_damage += hit.damage,
        }
    }

    pub fn add(&mut self, other: &Tally) {
        self.hull_hits += other.hull_hits;
        self.crit_hull_hits += other.crit_hull_hits;
        self.base_damage += other.base_damage;
        self.crit_base_damage += other.crit_base_damage;
        self.hull_damage += other.hull_damage;
        self.shield_damage += other.shield_damage;
        self.prevented_to_hull += other.prevented_to_hull;
    }

    /// Damage dealt per second, hull and shields together — the figure the
    /// tables call DPS and the one a build is judged by.
    ///
    /// This is the verdict's quantity. An earlier version led with
    /// `hull_potential_dps` instead, which is a different question and gave a
    /// different answer: see `docs/BUILD_DIFF.md` "What the grouped runs
    /// showed".
    pub fn total_dps(&self, duration_s: f64) -> f64 {
        if duration_s <= 0.0 {
            return 0.0;
        }
        (self.hull_damage + self.shield_damage) / duration_s
    }

    /// Damage that reached the hull, per second. What the five factors multiply
    /// out to, exactly.
    pub fn hull_dps(&self, duration_s: f64) -> f64 {
        if duration_s <= 0.0 {
            return 0.0;
        }
        self.hull_damage / duration_s
    }

    /// Damage dealt to shields, per second. Additive rather than factored: a
    /// shield line carries no base damage, so nothing about target mitigation
    /// can be read off it.
    pub fn shield_dps(&self, duration_s: f64) -> f64 {
        if duration_s <= 0.0 {
            return 0.0;
        }
        self.shield_damage / duration_s
    }

    /// What the hull would have taken with no shields in the way: what reached
    /// it plus what they absorbed.
    ///
    /// Kept because it is the right quantity for "how much hull-killing power
    /// did this deliver", and deliberately **not** the verdict's quantity: a
    /// build can raise it without raising DPS at all, which is what the real
    /// runs turned out to do.
    pub fn hull_potential_dps(&self, duration_s: f64) -> f64 {
        if duration_s <= 0.0 {
            return 0.0;
        }
        (self.hull_damage + self.prevented_to_hull) / duration_s
    }

    /// The five factors, or `None` when there is nothing to take them of.
    pub fn factors(&self, duration_s: f64) -> Option<Factors> {
        if self.hull_hits == 0 || self.base_damage <= 0.0 || duration_s <= 0.0 {
            return None;
        }
        let hull_hits = self.hull_hits as f64;
        let mean_base = self.base_damage / hull_hits;
        let non_crit_hits = self.hull_hits - self.crit_hull_hits;
        // With every hit a crit there is no neutral hit to divide by, and
        // inventing one would put a made-up number in a factor the report
        // presents as measured. The crit factor is stated as 1 instead and the
        // whole of the hit size sits in `neutral`, which is true of what was
        // seen even though it is not the split that was wanted.
        let neutral = if non_crit_hits == 0 {
            mean_base
        } else {
            (self.base_damage - self.crit_base_damage) / non_crit_hits as f64
        };
        let would_have_taken = self.hull_damage + self.prevented_to_hull;
        Some(Factors {
            cadence: hull_hits / duration_s,
            neutral,
            crit_multiplier: mean_base / neutral,
            efficiency: would_have_taken / self.base_damage,
            shield_passthrough: if would_have_taken > 0.0 {
                self.hull_damage / would_have_taken
            } else {
                0.0
            },
        })
    }
}

/// The five things a hull DPS difference can be made of. Their product is
/// `Tally::hull_dps`, exactly.
///
/// Five rather than four because the fourth used to be the last: the product
/// came to what the hull *would* have taken, which is not a figure the program
/// shows anywhere. `shield_passthrough` carries the rest of the way to damage
/// that actually landed, and what is left over — damage dealt to shields — is
/// an additive term rather than a factor, since a shield line has no base
/// damage to be a fraction of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Factors {
    /// Hull hits per second.
    pub cadence: f64,
    /// Base damage of a hit that did not crit.
    pub neutral: f64,
    /// What critting multiplied the average hit by: `1 + p * (severity - 1)`,
    /// taken as the ratio of the mean base hit to the neutral one, which is the
    /// same number without needing the severity separately.
    pub crit_multiplier: f64,
    /// `1 - target resistance`. Above 1 when the target was debuffed past zero.
    pub efficiency: f64,
    /// What share of the hull damage owed actually got through rather than
    /// being absorbed by a shield. Softening a target raises `efficiency`; it
    /// does nothing here, and a build whose shots land on full shields loses
    /// that gain again exactly at this factor.
    pub shield_passthrough: f64,
}

impl Factors {
    /// In the order `as_array` returns them, for the report to label a split by.
    pub const NAMES: [&'static str; 5] = [
        "fired more often",
        "each hit bigger before crits",
        "critted more, or harder",
        "targets softer",
        "got past shields",
    ];

    pub fn as_array(self) -> [f64; 5] {
        [
            self.cadence,
            self.neutral,
            self.crit_multiplier,
            self.efficiency,
            self.shield_passthrough,
        ]
    }

    /// Damage that reached the hull, per second.
    pub fn hull_dps(self) -> f64 {
        self.as_array().iter().product()
    }
}

/// Split the change in a product between its factors, so that the shares sum to
/// the change in the product exactly.
///
/// This is the Shapley value of the product game: each factor is credited with
/// the average of what it adds over every order in which the factors could have
/// been changed one at a time. It is the same choice already made for the
/// compare table's `ΔDPS breakdown` — `split_dps_difference` takes the two
/// shares at the midpoint of the pair, which for two factors is this formula —
/// generalised so that a third and a fourth factor can be added without a
/// cross term left over to attribute by hand.
///
/// The alternative, taking logs so the factors become additive in percentage
/// terms, is cheaper and was rejected: the shares then sum in log space and not
/// in DPS, and a waterfall that does not close is worse than none.
pub fn split_product_change<const N: usize>(before: [f64; N], after: [f64; N]) -> [f64; N] {
    let mut shares = [0.0; N];
    let factorial = |n: usize| (1..=n).map(|v| v as f64).product::<f64>();
    for i in 0..N {
        let mut share = 0.0;
        // Every subset of the other factors, as a bit pattern: those in it are
        // already at their `after` value when factor `i` moves, the rest are
        // still at `before`.
        for mask in 0u32..(1 << (N - 1)) {
            let mut product = 1.0;
            let mut in_subset = 0usize;
            let mut bit = 0;
            for (j, _) in before.iter().enumerate() {
                if j == i {
                    continue;
                }
                let moved = mask & (1 << bit) != 0;
                bit += 1;
                if moved {
                    in_subset += 1;
                    product *= after[j];
                } else {
                    product *= before[j];
                }
            }
            let weight =
                factorial(in_subset) * factorial(N - in_subset - 1) / factorial(N);
            share += weight * product * (after[i] - before[i]);
        }
        shares[i] = share;
    }
    shares
}

/// One first-level row of a run: what it is called and what it did.
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub tally: Tally,
}

/// One run as the diff reads it: its rows, and the sums over all of them.
#[derive(Clone, Debug)]
pub struct Run {
    /// What the reader called this run — the combat note, which is the only
    /// place a build has a name. Empty when they named nothing.
    pub label: String,
    pub duration_s: f64,
    pub rows: Vec<Row>,
    pub whole: Tally,
}

impl Run {
    /// Read one player's outgoing damage into rows, one per first-level group.
    ///
    /// The first level is where the reader's abilities are; below it are the
    /// procs and pets that belong to them, and those are summed into their
    /// parent rather than listed, because a build is swapped a slot at a time
    /// and a slot is a first-level row.
    pub fn of(
        group: &DamageGroup,
        names: &NameManager,
        hits: &HitsManager,
        duration_s: f64,
        label: String,
    ) -> Self {
        let mut rows: Vec<Row> = group
            .sub_groups()
            .values()
            .map(|sub| Row {
                name: sub.name().get(names).to_string(),
                tally: tally_of(sub, hits),
            })
            .collect();
        rows.sort_by(|a, b| {
            b.tally
                .total_dps(duration_s)
                .total_cmp(&a.tally.total_dps(duration_s))
        });
        let mut whole = Tally::default();
        for row in &rows {
            whole.add(&row.tally);
        }
        Run {
            label,
            duration_s,
            rows,
            whole,
        }
    }
}

/// Every hit under a group, its own and its sub-groups'.
///
/// Walks the tree rather than reading the group's own `hits`, because a branch
/// group's `hits` is a range into the manager that its children were pushed
/// into — correct for the analyzer's own use, and not something to rely on
/// holding one row's hits and nothing else.
fn tally_of(group: &DamageGroup, hits: &HitsManager) -> Tally {
    let mut tally = Tally::default();
    if group.sub_groups().is_empty() {
        for hit in group.hits.get(hits) {
            tally.add_hit(hit);
        }
    } else {
        for sub in group.sub_groups().values() {
            tally.add(&tally_of(sub, hits));
        }
    }
    tally
}

/// A row present in one run and not the other.
#[derive(Clone, Debug)]
pub struct OnlyIn {
    pub name: String,
    /// Which of the two runs has it.
    pub run: usize,
    pub total_dps: f64,
}

/// What the rows the two runs share agree about, on one factor.
#[derive(Clone, Debug)]
pub struct Cluster {
    pub factor: &'static str,
    pub median: f64,
    /// Every row's ratio, largest first, for the report to show its working.
    pub ratios: Vec<(String, f64)>,
    /// How many of them sit within the tolerance of the median.
    pub agreeing: usize,
    /// Largest ratio less smallest.
    pub spread: f64,
}

impl Cluster {
    /// Whether this reads as one effect acting on everything, rather than as
    /// rows that happen to have moved.
    ///
    /// Deliberately not a claim that any particular ability did it: see the
    /// module docs. And deliberately a claim about **this pair of runs** only —
    /// the rows share a fight and share targets, so their agreement establishes
    /// a common cause within the run and says nothing about whether the next
    /// run will look the same.
    pub fn is_global(&self, tolerance: f64, agreement: f64) -> bool {
        !self.ratios.is_empty()
            && (self.median - 1.0).abs() > tolerance
            && self.agreeing as f64 >= agreement * self.ratios.len() as f64
    }
}

/// The whole comparison of two runs.
pub struct BuildDiff {
    pub runs: [Run; 2],
    pub only_in: Vec<OnlyIn>,
    /// The rows both runs have, summed — what the factor split is taken of.
    pub common: [Tally; 2],
    pub common_factors: Option<[Factors; 2]>,
    /// Per factor, in `Factors::NAMES` order. Sums to the change in the common
    /// core's **hull** DPS exactly. The change in damage dealt to shields is
    /// not among them — it is additive, `shield_dps_change`.
    pub factor_shares: Option<[f64; 5]>,
    /// What the change in damage dealt to shields came to, per second. Added to
    /// the factor shares this is the whole change in DPS.
    pub shield_dps_change: f64,
    pub clusters: Vec<Cluster>,
}

impl BuildDiff {
    pub fn of(a: Run, b: Run, min_hits: u64) -> Self {
        let index = |run: &Run| -> FxHashMap<String, Tally> {
            run.rows
                .iter()
                .map(|row| (row.name.clone(), row.tally))
                .collect()
        };
        let (ia, ib) = (index(&a), index(&b));

        let mut only_in: Vec<OnlyIn> = Vec::new();
        for (run_i, (mine, theirs)) in [(&ia, &ib), (&ib, &ia)].into_iter().enumerate() {
            let duration = if run_i == 0 { a.duration_s } else { b.duration_s };
            for (name, tally) in mine {
                if !theirs.contains_key(name) {
                    only_in.push(OnlyIn {
                        name: name.clone(),
                        run: run_i,
                        total_dps: tally.total_dps(duration),
                    });
                }
            }
        }
        only_in.sort_by(|x, y| y.total_dps.total_cmp(&x.total_dps));

        let mut common = [Tally::default(); 2];
        for (name, ta) in &ia {
            let Some(tb) = ib.get(name) else { continue };
            common[0].add(ta);
            common[1].add(tb);
        }

        let common_factors = common[0]
            .factors(a.duration_s)
            .zip(common[1].factors(b.duration_s))
            .map(|(fa, fb)| [fa, fb]);
        let factor_shares = common_factors
            .map(|[fa, fb]| split_product_change(fa.as_array(), fb.as_array()));

        let clusters = clusters_of(&ia, &ib, a.duration_s, b.duration_s, min_hits);
        let shield_dps_change =
            common[1].shield_dps(b.duration_s) - common[0].shield_dps(a.duration_s);

        BuildDiff {
            runs: [a, b],
            only_in,
            common,
            common_factors,
            factor_shares,
            shield_dps_change,
            clusters,
        }
    }
}

impl BuildDiff {
    /// The whole thing as text: the account of the difference, then the working
    /// behind it. Written here rather than in the drawing code so it can be
    /// read against a real log before there is anything to draw.
    pub fn report(&self) -> String {
        let mut out = String::new();
        let [a, b] = &self.runs;
        let (pa, pb) = (
            a.whole.total_dps(a.duration_s),
            b.whole.total_dps(b.duration_s),
        );

        out.push_str("DPS, the figure the tables show\n");
        out.push_str(&format!(
            "  {:<28} {:>12.0}   {} rows, {:.0}s\n",
            a.label,
            pa,
            a.rows.len(),
            a.duration_s
        ));
        out.push_str(&format!(
            "  {:<28} {:>12.0}   {} rows, {:.0}s\n",
            b.label,
            pb,
            b.rows.len(),
            b.duration_s
        ));
        out.push_str(&format!(
            "  {:<28} {:>+12.0}   ({:+.1}%)\n\n",
            "difference",
            pb - pa,
            if pa > 0.0 { 100.0 * (pb - pa) / pa } else { 0.0 }
        ));

        out.push_str("Where it came from\n");
        for only in &self.only_in {
            let sign = if only.run == 1 { 1.0 } else { -1.0 };
            out.push_str(&format!(
                "  {:>+12.0}   {} (only in {})\n",
                sign * only.total_dps,
                only.name,
                self.runs[only.run].label
            ));
        }
        match self.factor_shares {
            Some(shares) => {
                for (share, name) in shares.iter().zip(Factors::NAMES) {
                    out.push_str(&format!("  {share:>+12.0}   {name} (rows both runs have)\n"));
                }
            }
            None => out.push_str("  (no row both runs have dealt hull damage)\n"),
        }
        out.push_str(&format!(
            "  {:>+12.0}   damage dealt to shields (rows both runs have)\n",
            self.shield_dps_change
        ));
        let accounted: f64 = self
            .only_in
            .iter()
            .map(|o| if o.run == 1 { o.total_dps } else { -o.total_dps })
            .sum::<f64>()
            + self.factor_shares.map(|s| s.iter().sum::<f64>()).unwrap_or(0.0)
            + self.shield_dps_change;
        out.push_str(&format!(
            "  {:>+12.0}   accounted for, against {:+.0} to explain\n\n",
            accounted,
            pb - pa
        ));

        if let Some([fa, fb]) = self.common_factors {
            out.push_str("The four factors, over the rows both runs have\n");
            for (i, name) in Factors::NAMES.iter().enumerate() {
                let (x, y) = (fa.as_array()[i], fb.as_array()[i]);
                let whole = if x != 0.0 { y / x } else { 1.0 };
                // The aggregate is damage-weighted, so it moves when the *mix*
                // of rows firing changes even if no row changed at all. The
                // rows' own median does not. Where the two disagree, the gap is
                // the mix, and saying so is the difference between a finding
                // and an artefact presented as one.
                let rows = self.clusters.get(i).map(|c| c.median).unwrap_or(1.0);
                let mix = (whole - rows).abs() > DEFAULT_TOLERANCE * rows.max(1e-9);
                out.push_str(&format!(
                    "  {name:<30} {x:>10.3} -> {y:<10.3} ({:+.1}% together, rows themselves \
                     {:+.1}%){}\n",
                    100.0 * (whole - 1.0),
                    100.0 * (rows - 1.0),
                    if mix {
                        "  <- mostly the mix of rows, not the rows"
                    } else {
                        ""
                    }
                ));
            }
            out.push('\n');
        }

        out.push_str("Did one thing move everything, or did rows move on their own?\n");
        for cluster in &self.clusters {
            if cluster.ratios.is_empty() {
                continue;
            }
            out.push_str(&format!(
                "  {:<30} median {:.3}x over {} rows, {} agreeing, spread {:.3}{}\n",
                cluster.factor,
                cluster.median,
                cluster.ratios.len(),
                cluster.agreeing,
                cluster.spread,
                if cluster.is_global(DEFAULT_TOLERANCE, DEFAULT_AGREEMENT) {
                    "   <- one effect on all of them"
                } else {
                    ""
                }
            ));
        }
        out
    }
}

/// Per factor, how the rows the runs share moved on it.
fn clusters_of(
    ia: &FxHashMap<String, Tally>,
    ib: &FxHashMap<String, Tally>,
    da: f64,
    db: f64,
    min_hits: u64,
) -> Vec<Cluster> {
    let mut per_factor: [Vec<(String, f64)>; 4] = Default::default();
    for (name, ta) in ia {
        let Some(tb) = ib.get(name) else { continue };
        if ta.hull_hits < min_hits || tb.hull_hits < min_hits {
            continue;
        }
        let (Some(fa), Some(fb)) = (ta.factors(da), tb.factors(db)) else {
            continue;
        };
        for (i, (before, after)) in fa.as_array().into_iter().zip(fb.as_array()).enumerate() {
            if before > 0.0 {
                per_factor[i].push((name.clone(), after / before));
            }
        }
    }

    per_factor
        .into_iter()
        .enumerate()
        .map(|(i, mut ratios)| {
            ratios.sort_by(|x, y| y.1.total_cmp(&x.1));
            let median = median(&ratios.iter().map(|(_, r)| *r).collect::<Vec<_>>());
            let agreeing = ratios
                .iter()
                .filter(|(_, r)| (r - median).abs() <= DEFAULT_TOLERANCE * median)
                .count();
            let spread = match (ratios.first(), ratios.last()) {
                (Some((_, high)), Some((_, low))) => high - low,
                _ => 0.0,
            };
            Cluster {
                factor: Factors::NAMES[i],
                median,
                ratios,
                agreeing,
                spread,
            }
        })
        .collect()
}

fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::BaseHit;

    fn hull(damage: f64, base: f64, critical: bool) -> Hit {
        let flags = if critical {
            ValueFlags::CRITICAL
        } else {
            ValueFlags::NONE
        };
        BaseHit::hull(damage, flags, base).to_hit(0)
    }

    /// The four shares are an account of the difference, so they have to come
    /// to it exactly whatever the numbers do — including when two factors move
    /// opposite ways and each share is larger than the difference itself.
    #[test]
    fn the_factor_shares_add_up_to_the_whole_difference() {
        for (before, after) in [
            ([10.0, 500.0, 1.5, 0.8], [12.0, 550.0, 1.6, 0.9]),
            ([10.0, 500.0, 1.5, 0.8], [20.0, 250.0, 1.5, 0.8]),
            ([10.0, 500.0, 1.5, 0.8], [10.0, 500.0, 1.5, 1.4]),
            ([1.0, 1.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]),
            ([10.0, 500.0, 1.5, 0.8], [0.0, 0.0, 1.0, 1.0]),
        ] {
            let shares = split_product_change(before, after);
            let whole: f64 = after.iter().product::<f64>() - before.iter().product::<f64>();
            assert!(
                (shares.iter().sum::<f64>() - whole).abs() < 1e-9,
                "{before:?} -> {after:?}: {shares:?} does not come to {whole}"
            );
        }
    }

    /// For two factors this must be the split the compare table already shows,
    /// or the same difference would be attributed two different ways in two
    /// places in one program.
    #[test]
    fn two_factors_agree_with_the_compare_table() {
        for (r1, m1, r2, m2) in [
            (10.0, 500.0, 12.0, 600.0),
            (10.0, 500.0, 20.0, 250.0),
            (0.0, 0.0, 7.0, 300.0),
        ] {
            let theirs = super::super::compare_table::split_dps_difference(r1, m1, r2, m2);
            let mine = split_product_change([r1, m1], [r2, m2]);
            assert!((mine[0] - theirs.rate).abs() < 1e-9, "rate: {mine:?}");
            assert!((mine[1] - theirs.size).abs() < 1e-9, "size: {mine:?}");
        }
    }


    /// Runs the diff over two combats of a real log and prints the report.
    ///
    /// Point `CLA_TEST_COMBATLOG` at a combatlog.log. With no
    /// `CLA_TEST_COMBATS` it lists the combats with their numbers and stops, so
    /// the two to compare can be picked; with `CLA_TEST_COMBATS=3,7` it reports
    /// on those two.
    ///
    /// `CLA_TEST_COMBATLOG=<path> CLA_TEST_COMBATS=3,7 \
    ///   cargo test build_diff_on_a_real_log -- --ignored --nocapture`
    #[test]
    #[ignore = "reads a real STO log"]
    fn build_diff_on_a_real_log() {
        use crate::analyzer::{Analyzer, settings::AnalysisSettings};
        use crate::app::damage_subset::metrics_duration;
        use crate::app::settings::Settings;

        let Some(path) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this");
            return;
        };
        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: path.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .expect("the log opens");
        analyzer.update();
        let combats = analyzer.result();

        let picked: Vec<usize> = std::env::var("CLA_TEST_COMBATS")
            .ok()
            .map(|v| v.split(',').filter_map(|p| p.trim().parse().ok()).collect())
            .unwrap_or_default();
        if picked.len() != 2 {
            println!("{} combats; pass two of these numbers in CLA_TEST_COMBATS", combats.len());
            for (i, combat) in combats.iter().enumerate() {
                println!(
                    "  {:>3}  {:<44} {}  {} players",
                    i,
                    combat.name(),
                    combat.active_time.start.format("%Y-%m-%d %H:%M"),
                    combat.players.len()
                );
            }
            return;
        }

        // The note is where a build has a name — the log carries no loadout, so
        // the label the reader typed is the only thing that says which build a
        // run was. This is the grouping the design rests on, exercised here.
        let notes = Settings::load_or_default().combat_notes;
        let run_of = |index: usize| {
            let combat = &combats[index];
            let note = notes.get(&crate::app::settings::CombatNotes::key(combat));
            let (_, player) = combat
                .players
                .iter()
                .max_by(|(_, x), (_, y)| x.damage_out.dps.all.total_cmp(&y.damage_out.dps.all))
                .expect("a player");
            Run::of(
                &player.damage_out,
                &combat.name_manager,
                &combat.hits_manger,
                metrics_duration(&player.combat_time),
                if note.is_empty() {
                    format!("#{index} {}", combat.active_time.start.format("%m-%d %H:%M"))
                } else {
                    format!("#{index} {note}")
                },
            )
        };

        let diff = BuildDiff::of(run_of(picked[0]), run_of(picked[1]), MIN_HITS_FOR_A_RATIO);
        println!("\n{}", diff.report());
    }

    /// A change in one factor alone is credited to that factor and to no other.
    #[test]
    fn a_change_in_one_factor_lands_on_that_factor() {
        let shares = split_product_change([10.0, 500.0, 1.0, 0.5], [10.0, 500.0, 1.0, 0.6]);
        assert_eq!([0.0, 0.0, 0.0], [shares[0], shares[1], shares[2]]);
        assert!((shares[3] - 500.0).abs() < 1e-9, "10 x 500 x 0.1 = 500");
    }

    /// The crit split is taken on base damage, because that is where the game
    /// puts crit severity — a crit-flagged hit carries a larger base figure,
    /// not merely a larger dealt one.
    #[test]
    fn the_crit_multiplier_is_the_ratio_of_mean_hit_to_neutral_hit() {
        let mut tally = Tally::default();
        for _ in 0..3 {
            tally.add_hit(&hull(100.0, 200.0, false));
        }
        tally.add_hit(&hull(250.0, 500.0, true));

        let factors = tally.factors(2.0).expect("four hits");
        assert_eq!(2.0, factors.cadence, "four hull hits over two seconds");
        assert_eq!(200.0, factors.neutral, "the three that did not crit");
        // Mean base is (200*3 + 500) / 4 = 275.
        assert!((factors.crit_multiplier - 275.0 / 200.0).abs() < 1e-9);
    }

    /// Efficiency counts what the shields stopped as damage the hull was owed,
    /// which is what makes it the target's own mitigation and not the shields'.
    #[test]
    fn efficiency_counts_what_the_shields_stopped() {
        let mut tally = Tally::default();
        tally.add_hit(&hull(600.0, 1_000.0, false));
        tally.add_hit(&BaseHit::shield(50.0, ValueFlags::NONE, 200.0).to_hit(0));

        let factors = tally.factors(1.0).expect("a hull hit");
        assert!(
            (factors.efficiency - 0.8).abs() < 1e-9,
            "600 through plus 200 stopped, of 1000: {}",
            factors.efficiency
        );
    }

    /// A shield drain has neither a hull side nor a base damage, so it must not
    /// move any factor. It used to be tempting to count its damage somewhere.
    #[test]
    fn a_shield_drain_moves_nothing() {
        let mut tally = Tally::default();
        tally.add_hit(&hull(600.0, 1_000.0, false));
        let without = tally.factors(1.0).unwrap();

        tally.add_hit(&BaseHit::shield_drain(400.0, ValueFlags::NONE).to_hit(0));
        assert_eq!(without, tally.factors(1.0).unwrap());
    }

    /// Rows that all moved by the same multiplier read as one effect; rows that
    /// scattered do not, however far the median is from 1.
    #[test]
    fn agreement_across_rows_is_what_makes_a_lift_global() {
        let cluster = |ratios: &[f64]| Cluster {
            factor: "targets softer",
            median: median(ratios),
            ratios: ratios
                .iter()
                .enumerate()
                .map(|(i, r)| (format!("row {i}"), *r))
                .collect(),
            agreeing: ratios
                .iter()
                .filter(|r| (**r - median(ratios)).abs() <= DEFAULT_TOLERANCE * median(ratios))
                .count(),
            spread: 0.0,
        };

        assert!(
            cluster(&[1.20, 1.22, 1.19, 1.21, 1.20]).is_global(DEFAULT_TOLERANCE, DEFAULT_AGREEMENT),
            "five rows within a few percent of 1.2"
        );
        assert!(
            !cluster(&[1.20, 0.80, 1.60, 1.05, 1.35]).is_global(DEFAULT_TOLERANCE, DEFAULT_AGREEMENT),
            "the same median, but nothing agrees with it"
        );
        assert!(
            !cluster(&[1.01, 1.00, 1.00, 1.01, 0.99]).is_global(DEFAULT_TOLERANCE, DEFAULT_AGREEMENT),
            "perfect agreement that nothing happened"
        );
    }
}
