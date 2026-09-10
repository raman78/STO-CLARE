use std::{
    borrow::{Borrow, BorrowMut},
    path::Path,
};

use serde::*;

use super::parser::*;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnalysisSettings {
    pub combatlog_file: String,
    pub combat_separation_time_seconds: f64,
    pub indirect_source_grouping_revers_rules: Vec<MatchRule>,
    pub custom_group_rules: Vec<RulesGroup>,
    #[serde(default)]
    pub damage_out_exclusion_rules: Vec<MatchRule>,
    pub combat_name_rules: Vec<CombatNameRule>,
    // Linux: merge STO's rotating combatlog_<timestamp>.log files into one
    // combatlog.log and read that (no-op elsewhere). See app::log_consolidation.
    #[serde(default = "consolidate_combatlog_default")]
    pub consolidate_combatlog: bool,
}

fn consolidate_combatlog_default() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct CombatNameRule {
    pub name_rule: RulesGroup,
    pub additional_info_rules: Vec<RulesGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MatchRule {
    pub aspect: MatchAspect,
    pub expression: String,
    pub method: MatchMethod,
    pub enabled: bool,
}

// The variant names are what the settings file stores, so they are read back
// from every existing installation and cannot be shortened.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchAspect {
    SourceOrTargetName,
    SourceOrTargetUniqueName,
    IndirectSourceName,
    IndirectUniqueSourceName,
    #[default]
    DamageOrHealName,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MatchMethod {
    #[default]
    Equals,
    StartsWith,
    EndsWith,
    Contains,
    /// A pattern with wildcards in it — see [`wildcard_matches`]. Added after
    /// the four above, so a settings file written before it exists never names
    /// it and reads back unchanged.
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesGroup {
    pub name: String,
    pub rules: Vec<MatchRule>,
    pub enabled: bool,
}

impl AnalysisSettings {
    pub fn combatlog_file(&self) -> &Path {
        Path::new(&self.combatlog_file)
    }
}

impl RulesGroup {
    pub fn matches_source_or_target_names<'a>(
        &self,
        mut names: impl Iterator<Item = &'a str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        names.any(|n| {
            self.rules
                .iter()
                .any(|r| r.matches_source_or_target_name(n))
        })
    }

    pub fn matches_source_or_target_unique_names<'a>(
        &self,
        mut names: impl Iterator<Item = &'a str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        names.any(|n| {
            self.rules
                .iter()
                .any(|r| r.matches_source_or_target_unique_name(n))
        })
    }

    pub fn matches_indirect_source_names<'a>(
        &self,
        mut names: impl Iterator<Item = &'a str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        names.any(|n| self.rules.iter().any(|r| r.matches_indirect_source_name(n)))
    }

    pub fn matches_indirect_source_unique_names<'a>(
        &self,
        mut names: impl Iterator<Item = &'a str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        names.any(|n| {
            self.rules
                .iter()
                .any(|r| r.matches_indirect_source_unique_name(n))
        })
    }

    pub fn matches_damage_or_heal_names<'a>(
        &self,
        mut names: impl Iterator<Item = &'a str>,
    ) -> bool {
        if !self.enabled {
            return false;
        }

        names.any(|n| self.rules.iter().any(|r| r.matches_damage_or_heal_name(n)))
    }

    /// How precisely this group pins down `record`, or `None` when none of its
    /// conditions matches it.
    ///
    /// A group matches when any one of its conditions does, so the group is as
    /// precise as its best-fitting condition. See [`Specificity`].
    pub fn specificity_for_record(&self, record: &Record) -> Option<Specificity> {
        if !self.enabled {
            return None;
        }

        self.rules
            .iter()
            .filter_map(|rule| rule.specificity_for_record(record))
            .max()
    }

    /// The same, for one name read as one aspect — what the Analysis tab has to
    /// hand when it is looking for two rules claiming the same effect.
    pub fn specificity_for(&self, aspect: MatchAspect, name: &str) -> Option<Specificity> {
        if !self.enabled {
            return None;
        }

        self.rules
            .iter()
            .filter_map(|rule| rule.specificity_for(aspect, name))
            .max()
    }
}

/// Which of `groups` claims `record`, when more than one does.
///
/// The most precisely fitting group wins, never the one that happens to be
/// first in the list — see [`Specificity`] for what "precise" is measured as.
/// A rule is therefore worth the same wherever it sits, which is what lets the
/// list be sorted by name and lets a group be imported from someone else's file
/// without its position deciding whether it does anything.
///
/// Two groups fitting equally well are settled by name, alphabetically, so the
/// answer is the same on every machine and in every reading of the same log.
/// The Analysis tab flags that case with a ⚠ rather than leaving it to be
/// discovered in a table: two rules fitting a name equally well is nearly
/// always one of them being meant to be narrower.
pub fn most_specific_match<'a>(
    groups: impl IntoIterator<Item = &'a RulesGroup>,
    record: &Record,
) -> Option<&'a RulesGroup> {
    groups
        .into_iter()
        .filter_map(|group| Some((group.specificity_for_record(record)?, group)))
        .max_by(|(left, left_group), (right, right_group)| {
            left.cmp(right)
                // Reversed, so that the *earlier* name wins the tie: `max_by`
                // keeps the largest, and the largest of two reversed names is
                // the one that sorts first.
                .then_with(|| right_group.name.cmp(&left_group.name))
        })
        .map(|(_, group)| group)
}

/// How precisely a condition pinned down the name it matched.
///
/// Compared to decide which of several rules claiming the same record wins,
/// so that the answer does not depend on the order the rules happen to sit in.
/// The order they sit in is alphabetical, and a list a reader can find a rule
/// in is not a list whose order may also decide what the program does.
///
/// Read in two parts, most significant first:
///
/// 1. **Literal characters.** How many characters of the name the pattern
///    actually spells out. `Contains "Phaser"` pins six; `Wildcard "Quad*Cannons"`
///    pins eleven, since `*` spells nothing. A `?` counts for nothing either: it
///    fixes a position but says nothing about what is in it.
/// 2. **Whether the whole name is covered.** `Equals` always does, a wildcard
///    pattern does by definition, and the other three only when their text is
///    the entire name.
/// 3. **Whether the method admits nothing else.** Only `Equals` does. This is
///    what separates `Equals "Quad Cannons"` from `Contains "Quad Cannons"`,
///    which spell out the same characters and cover the same whole name when
///    read against that name — but the first was written to catch one ability
///    and the second to catch a family of them.
///
/// Taken in any other order the ranking breaks: put coverage first and
/// `Wildcard "*"` — which covers every name and spells out nothing — beats
/// every carefully written rule in the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Specificity {
    literal_characters: usize,
    covers_the_whole_name: bool,
    admits_nothing_else: bool,
}

impl MatchRule {
    pub fn matches_record(&self, record: &Record) -> bool {
        if !self.enabled {
            return false;
        }

        match self.aspect {
            MatchAspect::SourceOrTargetName => {
                self.method
                    .check_match_or_false(&self.expression, record.source.name())
                    || self
                        .method
                        .check_match_or_false(&self.expression, record.target.name())
            }
            MatchAspect::SourceOrTargetUniqueName => {
                self.method
                    .check_match_or_false(&self.expression, record.source.unique_name())
                    || self
                        .method
                        .check_match_or_false(&self.expression, record.target.unique_name())
            }
            MatchAspect::IndirectSourceName => self
                .method
                .check_match_or_false(&self.expression, record.indirect_source.name()),
            MatchAspect::IndirectUniqueSourceName => self
                .method
                .check_match_or_false(&self.expression, record.indirect_source.unique_name()),
            MatchAspect::DamageOrHealName => self
                .method
                .check_match(&self.expression, &record.value_name),
        }
    }

    pub fn matches_source_or_target_name(&self, name: &str) -> bool {
        if !self.enabled || self.aspect != MatchAspect::SourceOrTargetName {
            return false;
        }

        self.method.check_match(&self.expression, name)
    }

    pub fn matches_source_or_target_unique_name(&self, name: &str) -> bool {
        if !self.enabled || self.aspect != MatchAspect::SourceOrTargetUniqueName {
            return false;
        }

        self.method.check_match(&self.expression, name)
    }

    pub fn matches_indirect_source_name(&self, name: &str) -> bool {
        if !self.enabled || self.aspect != MatchAspect::IndirectSourceName {
            return false;
        }

        self.method.check_match(&self.expression, name)
    }

    pub fn matches_indirect_source_unique_name(&self, name: &str) -> bool {
        if !self.enabled || self.aspect != MatchAspect::IndirectUniqueSourceName {
            return false;
        }

        self.method.check_match(&self.expression, name)
    }

    pub fn matches_damage_or_heal_name(&self, name: &str) -> bool {
        if !self.enabled || self.aspect != MatchAspect::DamageOrHealName {
            return false;
        }

        self.method.check_match(&self.expression, name)
    }

    /// How precisely this condition pins down `name` read as `aspect`, or
    /// `None` when it is switched off, is about a different aspect, or simply
    /// does not match. See [`Specificity`].
    pub fn specificity_for(&self, aspect: MatchAspect, name: &str) -> Option<Specificity> {
        if !self.enabled || self.aspect != aspect {
            return None;
        }

        self.method.specificity(&self.expression, name)
    }

    /// How precisely this condition pins down `record`, or `None` when it does
    /// not match it at all.
    ///
    /// Where an aspect reads two names — source *or* target — the better of the
    /// two answers is the one reported: the condition matched on whichever it
    /// matched on, and that is how precisely it did so.
    pub fn specificity_for_record(&self, record: &Record) -> Option<Specificity> {
        let against = |value: &str| self.specificity_for(self.aspect, value);
        let or_none = |value: Option<&str>| value.and_then(&against);
        match self.aspect {
            MatchAspect::SourceOrTargetName => {
                or_none(record.source.name()).max(or_none(record.target.name()))
            }
            MatchAspect::SourceOrTargetUniqueName => {
                or_none(record.source.unique_name()).max(or_none(record.target.unique_name()))
            }
            MatchAspect::IndirectSourceName => or_none(record.indirect_source.name()),
            MatchAspect::IndirectUniqueSourceName => or_none(record.indirect_source.unique_name()),
            MatchAspect::DamageOrHealName => against(&record.value_name),
        }
    }
}

impl MatchAspect {
    pub const fn display(self) -> &'static str {
        match self {
            MatchAspect::SourceOrTargetName => "Source or Target Name",
            MatchAspect::SourceOrTargetUniqueName => "Source or Target Unique Name",
            MatchAspect::IndirectSourceName => "Indirect Source Name",
            MatchAspect::DamageOrHealName => "Damage / Heal Name",
            MatchAspect::IndirectUniqueSourceName => "Indirect Source Unique Name",
        }
    }
}

impl MatchMethod {
    fn check_match(&self, expression: &str, value: &str) -> bool {
        match self {
            MatchMethod::Equals => value == expression,
            MatchMethod::StartsWith => value.starts_with(expression),
            MatchMethod::EndsWith => value.ends_with(expression),
            MatchMethod::Contains => value.contains(expression),
            MatchMethod::Wildcard => wildcard_matches(expression, value),
        }
    }

    /// How precisely `expression` pins down `value` under this method, or
    /// `None` when it does not match it. See [`Specificity`].
    fn specificity(&self, expression: &str, value: &str) -> Option<Specificity> {
        if !self.check_match(expression, value) {
            return None;
        }
        let (literal_characters, covers_the_whole_name) = match self {
            MatchMethod::Equals => (expression.chars().count(), true),
            MatchMethod::Wildcard => (
                expression
                    .chars()
                    .filter(|c| !matches!(c, '*' | '%' | '?'))
                    .count(),
                true,
            ),
            MatchMethod::StartsWith | MatchMethod::EndsWith | MatchMethod::Contains => (
                expression.chars().count(),
                expression.chars().count() == value.chars().count(),
            ),
        };
        Some(Specificity {
            literal_characters,
            covers_the_whole_name,
            admits_nothing_else: matches!(self, MatchMethod::Equals),
        })
    }

    fn check_match_or_false(&self, expression: &str, value: Option<&str>) -> bool {
        match value {
            Some(value) => self.check_match(expression, value),
            None => false,
        }
    }

    pub const fn display(self) -> &'static str {
        match self {
            MatchMethod::Equals => "Equals",
            MatchMethod::StartsWith => "Starts with",
            MatchMethod::EndsWith => "Ends with",
            MatchMethod::Contains => "Contains",
            MatchMethod::Wildcard => "Wildcard",
        }
    }

    /// What the method does, in the words shown beside the picker.
    pub const fn explanation(self) -> &'static str {
        match self {
            MatchMethod::Equals => "the name is exactly this text",
            MatchMethod::StartsWith => "the name begins with this text",
            MatchMethod::EndsWith => "the name ends with this text",
            MatchMethod::Contains => "this text appears anywhere in the name",
            MatchMethod::Wildcard => {
                "* or % stands for any run of characters, ? for exactly one; \
                 the pattern has to cover the whole name"
            }
        }
    }
}

/// Whether `value` matches `pattern`, where `*` and `%` each stand for any run
/// of characters (including none) and `?` for exactly one.
///
/// Both wildcards mean the same thing on purpose: a player who knows file
/// patterns writes `*Cannons*` and one who knows SQL writes `%Cannons%`, and
/// neither should have to find out which of the two this program happens to
/// take. There is no escape character — a name with a literal `*` in it is
/// matched with `Contains` instead.
///
/// The pattern covers the whole name, so a pattern with no wildcard in it
/// behaves like `Equals`. Case matters, as it does for every other method.
///
/// Linear in the length of the name: `star` remembers the last wildcard and
/// what it had consumed, so a run that turns out too short is given one more
/// character rather than the whole pattern being tried again from the start.
pub fn wildcard_matches(pattern: &str, value: &str) -> bool {
    let is_any_run = |c: char| c == '*' || c == '%';
    let pattern: Vec<char> = pattern.chars().collect();
    let value: Vec<char> = value.chars().collect();
    let (mut p, mut v) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;

    while v < value.len() {
        match pattern.get(p) {
            Some(&c) if is_any_run(c) => {
                star = Some((p, v));
                p += 1;
            }
            Some('?') => {
                p += 1;
                v += 1;
            }
            Some(&c) if c == value[v] => {
                p += 1;
                v += 1;
            }
            // Nothing here matches: hand one more character to the last run and
            // carry on from just after it. With no run behind us there is
            // nowhere to go back to.
            _ => match star {
                Some((star_p, star_v)) => {
                    p = star_p + 1;
                    v = star_v + 1;
                    star = Some((star_p, star_v + 1));
                }
                None => return false,
            },
        }
    }

    // The name is used up; what is left of the pattern may only be empty runs.
    pattern[p..].iter().all(|&c| is_any_run(c))
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            combatlog_file: Default::default(),
            // Matches the OSCR server, which splits uploaded logs on a 60s gap
            // with that value hard-coded. A longer window here would hand the
            // ladder a slice containing more than it will actually read.
            combat_separation_time_seconds: 60.0,
            indirect_source_grouping_revers_rules: Default::default(),
            custom_group_rules: Default::default(),
            damage_out_exclusion_rules: Default::default(),
            combat_name_rules: Default::default(),
            consolidate_combatlog: true,
        }
    }
}

impl Default for MatchRule {
    fn default() -> Self {
        Self {
            enabled: true,
            aspect: Default::default(),
            expression: Default::default(),
            method: Default::default(),
        }
    }
}

impl Default for RulesGroup {
    fn default() -> Self {
        Self {
            name: Default::default(),
            rules: Default::default(),
            enabled: true,
        }
    }
}

#[cfg(test)]
mod specificity_tests {
    use super::*;

    /// How precisely one method's text pins down a name, or nothing when it
    /// does not match it at all.
    fn fit(method: MatchMethod, expression: &str, value: &str) -> Option<Specificity> {
        method.specificity(expression, value)
    }

    /// The real case this replaced first-match-wins for: a general pattern and
    /// a narrower one for the same family of weapons. The narrower one is what
    /// its author wrote to say something, and it has to outrank the other.
    #[test]
    fn the_narrower_of_two_patterns_fits_better() {
        let ability = "Phaser Wide Angle Dual Heavy Beam Bank - Overload I";
        let general = fit(MatchMethod::StartsWith, "Phaser Wide Angle", ability);
        let narrow = fit(
            MatchMethod::StartsWith,
            "Phaser Wide Angle Dual Heavy Beam Bank",
            ability,
        );
        assert!(narrow > general, "{narrow:?} should outrank {general:?}");
    }

    /// An exact match beats the same text used loosely.
    #[test]
    fn an_exact_match_beats_the_same_text_as_a_fragment() {
        let name = "Quad Cannons";
        assert!(
            fit(MatchMethod::Equals, name, name) > fit(MatchMethod::Contains, name, name),
            "Equals and Contains spell out the same characters, and the exact \
             one covers the whole name"
        );
    }

    /// A wildcard is worth the characters it spells out, not the names it can
    /// reach. Ranked the other way round, `*` — which covers every name and
    /// says nothing — would outrank every carefully written rule in the list.
    #[test]
    fn a_catch_all_wildcard_loses_to_anything_that_says_something() {
        let ability = "Terran Task Force Phaser Beam Array";
        assert!(
            fit(MatchMethod::Contains, "Phaser", ability)
                > fit(MatchMethod::Wildcard, "*", ability)
        );
        assert!(
            fit(MatchMethod::Contains, "Phaser", ability)
                > fit(MatchMethod::Wildcard, "%", ability)
        );
    }

    #[test]
    fn a_wildcard_is_worth_its_literal_characters() {
        let ability = "Quad Disruptor Cannons - Rapid Fire III";
        assert!(
            fit(MatchMethod::Wildcard, "Quad*Cannons*", ability)
                > fit(MatchMethod::Contains, "Quad", ability),
            "eleven spelled-out characters against four"
        );
    }

    /// A `?` fixes a position but says nothing about what stands in it, so it
    /// buys no precision. Both patterns here spell out `Mk ` and `II`; only the
    /// wildcard between them differs.
    #[test]
    fn a_single_character_wildcard_counts_for_nothing() {
        assert_eq!(
            fit(MatchMethod::Wildcard, "Mk ?II", "Mk XII"),
            fit(MatchMethod::Wildcard, "Mk *II", "Mk XII"),
        );
    }

    #[test]
    fn a_pattern_that_does_not_match_has_no_fit_at_all() {
        assert_eq!(None, fit(MatchMethod::Contains, "Torpedo", "Quad Cannons"));
    }

    /// Two conditions can fit equally well; the caller settles that by name.
    #[test]
    fn equal_patterns_fit_equally() {
        let ability = "Terran Task Force Phaser Beam Array";
        assert_eq!(
            fit(MatchMethod::Contains, "Phaser", ability),
            fit(MatchMethod::Contains, "Phaser", ability)
        );
    }

    /// A group is as precise as its best-fitting condition, since any one of
    /// them matching is enough for the group to match.
    #[test]
    fn a_group_is_worth_its_best_condition() {
        let condition = |method, expression: &str| MatchRule {
            aspect: MatchAspect::DamageOrHealName,
            expression: expression.to_string(),
            method,
            enabled: true,
        };
        let group = RulesGroup {
            name: "Quad Cannons".to_string(),
            enabled: true,
            rules: vec![
                condition(MatchMethod::Contains, "Quad"),
                condition(MatchMethod::StartsWith, "Quad Disruptor Cannons"),
            ],
        };
        let ability = "Quad Disruptor Cannons - Rapid Fire III";

        let best = group
            .rules
            .iter()
            .filter_map(|rule| fit(rule.method, &rule.expression, ability))
            .max();
        assert_eq!(
            fit(MatchMethod::StartsWith, "Quad Disruptor Cannons", ability),
            best
        );
    }
}

#[cfg(test)]
mod wildcard_tests {
    use super::*;

    /// The two spellings of "any run of characters" are the same wildcard, so a
    /// player who reaches for either gets the same answer.
    #[test]
    fn star_and_percent_mean_the_same_thing() {
        for pattern in ["*Cannons*", "%Cannons%", "*Cannons%", "%Cannons*"] {
            assert!(
                wildcard_matches(pattern, "Quad Disruptor Cannons - Rapid Fire III"),
                "{pattern} should have matched"
            );
        }
    }

    #[test]
    fn a_run_matches_nothing_at_all() {
        assert!(wildcard_matches("*Quad Cannons*", "Quad Cannons"));
        assert!(wildcard_matches("Quad*", "Quad"));
    }

    #[test]
    fn a_question_mark_stands_for_exactly_one_character() {
        assert!(wildcard_matches("Mk ?II", "Mk XII"));
        assert!(!wildcard_matches("Mk ?II", "Mk II"));
        assert!(!wildcard_matches("Mk ?II", "Mk XIII"));
    }

    /// Without a wildcard the pattern has to cover the whole name — otherwise
    /// `Wildcard` would quietly behave like `Contains` and a rule meant to pick
    /// out one ability would take every ability whose name holds that word.
    #[test]
    fn a_pattern_without_wildcards_is_an_exact_match() {
        assert!(wildcard_matches("Quad Cannons", "Quad Cannons"));
        assert!(!wildcard_matches("Quad", "Quad Cannons"));
        assert!(!wildcard_matches("Cannons", "Quad Cannons"));
    }

    #[test]
    fn a_pattern_that_does_not_fit_is_refused() {
        assert!(!wildcard_matches("*Phaser*", "Quad Disruptor Cannons"));
        assert!(!wildcard_matches("Quad*Torpedo", "Quad Disruptor Cannons"));
    }

    /// The run has to be able to give up ground it took too eagerly: the first
    /// `Cannons` here is not the one that lets the rest of the pattern fit.
    #[test]
    fn a_run_gives_back_what_it_took_too_early() {
        assert!(wildcard_matches(
            "*Cannons*III",
            "Cannons Cannons - Rapid Fire III"
        ));
        assert!(wildcard_matches("*a*b", "aaab"));
    }

    #[test]
    fn an_empty_pattern_matches_only_an_empty_name() {
        assert!(wildcard_matches("", ""));
        assert!(!wildcard_matches("", "Quad Cannons"));
        assert!(wildcard_matches("*", ""));
    }

    /// Case is significant, as it is for every other match method.
    #[test]
    fn case_matters_as_it_does_elsewhere() {
        assert!(!wildcard_matches("*cannons*", "Quad Disruptor Cannons"));
    }

    /// Reached through the enum, which is how the analyzer asks.
    #[test]
    fn the_method_routes_to_the_matcher() {
        let rule = MatchRule {
            aspect: MatchAspect::DamageOrHealName,
            expression: "Quad*Cannons*".to_string(),
            method: MatchMethod::Wildcard,
            enabled: true,
        };
        assert!(rule.matches_damage_or_heal_name("Quad Disruptor Cannons - Rapid Fire III"));
        assert!(rule.matches_damage_or_heal_name("Quad Phaser Cannons"));
        assert!(!rule.matches_damage_or_heal_name("Terran Task Force Phaser Beam Array"));
    }

    /// A settings file written before this variant existed must still read, and
    /// must not be turned into a wildcard rule by accident.
    #[test]
    fn an_older_settings_file_still_reads_back() {
        let stored = r#"{"aspect":"DamageOrHealName","expression":"Quad","method":"Contains","enabled":true}"#;
        let rule: MatchRule = serde_json::from_str(stored).unwrap();
        assert_eq!(MatchMethod::Contains, rule.method);
    }
}

impl Borrow<RulesGroup> for CombatNameRule {
    fn borrow(&self) -> &RulesGroup {
        &self.name_rule
    }
}

impl BorrowMut<RulesGroup> for CombatNameRule {
    fn borrow_mut(&mut self) -> &mut RulesGroup {
        &mut self.name_rule
    }
}
