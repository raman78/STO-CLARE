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
    // The four rule sets live in their own file from 2.8 on — see `RuleSets`.
    // Still *read* from the settings, because that is where every existing
    // installation has them and that is what makes the move a no-op for the
    // reader; no longer written there, so the old copy goes on the first save
    // rather than lingering as a second, diverging source of the same truth.
    #[serde(default, skip_serializing)]
    pub indirect_source_grouping_revers_rules: Vec<MatchRule>,
    #[serde(default, skip_serializing)]
    pub custom_group_rules: Vec<RulesGroup>,
    #[serde(default, skip_serializing)]
    pub damage_out_exclusion_rules: Vec<MatchRule>,
    #[serde(default, skip_serializing)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RulesGroup {
    pub name: String,
    pub rules: Vec<MatchRule>,
    pub enabled: bool,
}

/// The Analysis tab's four rule sets, as one file.
///
/// Kept apart from the settings so that a set of rules is something a player
/// can copy to another machine or hand to someone else without also handing
/// over their log path, their window size and their handle. It is also what
/// Export and Import write and read, so the file in the config directory and
/// the file a player passes around are the same shape.
///
/// Written as TOML rather than JSON, because this one is meant to be opened and
/// read: a rule becomes a named section with four plain `key = value` lines,
/// instead of a nest of braces and quoted keys. The settings file stays JSON —
/// nobody reads that one by hand.
///
/// The field order here is the order it is written in, and `version` has to
/// stay first: TOML puts plain values before tables, and the writer will not
/// emit a value after a table has been opened.
///
/// `version` is written and checked on the way in, so a file from a newer build
/// is refused rather than half-read. That check is only worth anything if the
/// number is raised whenever the shape changes, which is what
/// `the_shape_of_the_rules_file_is_pinned` is there to force: unknown keys are
/// ignored on the way in, so a renamed field would otherwise be dropped in
/// silence and read as "the player never set it".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleSets {
    pub version: u32,
    #[serde(default)]
    pub combat_name_rules: Vec<CombatNameRule>,
    #[serde(default)]
    pub indirect_source_grouping_revers_rules: Vec<MatchRule>,
    #[serde(default)]
    pub custom_group_rules: Vec<RulesGroup>,
    #[serde(default)]
    pub damage_out_exclusion_rules: Vec<MatchRule>,
}

/// The shape written today. A file carrying a higher number was written by a
/// newer build and is refused rather than half-read.
pub const RULES_FILE_VERSION: u32 = 1;

impl Default for RuleSets {
    fn default() -> Self {
        Self {
            version: RULES_FILE_VERSION,
            combat_name_rules: Default::default(),
            indirect_source_grouping_revers_rules: Default::default(),
            custom_group_rules: Default::default(),
            damage_out_exclusion_rules: Default::default(),
        }
    }
}

/// What went wrong reading a rules file, in the words the dialog shows.
///
/// A rules file that cannot be read is never treated as an empty one: that
/// would silently throw away every rule its owner had written, and look
/// exactly like a fresh installation. The caller keeps what it had and says so.
#[derive(Debug)]
pub enum RulesFileError {
    Unreadable(std::io::Error),
    /// The file parsed as something, but not as a set of rules — or did not
    /// parse at all. Carries the reason in the words the format's own parser
    /// used, which for TOML names the line and what it expected there.
    NotRules(String),
    FromANewerVersion {
        found: u32,
        understood: u32,
    },
}

impl std::fmt::Display for RulesFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable(e) => write!(f, "the file could not be read: {e}"),
            // The reason goes last because a TOML error is several lines of its
            // own, ending in a caret under the offending character. Anything
            // written after that reads as part of the diagram.
            Self::NotRules(e) => write!(
                f,
                "This does not look like a rules file. A rules file is the one Export \
                 writes; a settings file or a combat log will not do. The reader stopped \
                 here:\n\n{e}"
            ),
            Self::FromANewerVersion { found, understood } => write!(
                f,
                "this file was written by a newer version of STO-CLARE (format {found}; \
                 this build understands {understood}). Update, and it will read."
            ),
        }
    }
}

impl RuleSets {
    pub fn path() -> Option<std::path::PathBuf> {
        Some(crate::helpers::paths::config_dir()?.join(crate::helpers::paths::RULES_FILE_NAME))
    }

    pub fn read(path: &Path) -> Result<Self, RulesFileError> {
        let data = std::fs::read_to_string(path).map_err(RulesFileError::Unreadable)?;
        let sets: Self =
            toml::from_str(&data).map_err(|e| RulesFileError::NotRules(e.to_string()))?;
        if sets.version > RULES_FILE_VERSION {
            return Err(RulesFileError::FromANewerVersion {
                found: sets.version,
                understood: RULES_FILE_VERSION,
            });
        }
        Ok(sets)
    }

    pub fn write(&self, path: &Path) -> Result<(), RulesFileError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(RulesFileError::Unreadable)?;
        }
        let data =
            toml::to_string_pretty(self).map_err(|e| RulesFileError::NotRules(e.to_string()))?;
        std::fs::write(path, data).map_err(RulesFileError::Unreadable)
    }

    /// How many rules this holds, per set, for a dialog that has to say what an
    /// import is about to bring in.
    pub fn summary(&self) -> String {
        format!(
            "{} combat name, {} source reversal, {} custom grouping, {} damage exclusion",
            self.combat_name_rules.len(),
            self.indirect_source_grouping_revers_rules.len(),
            self.custom_group_rules.len(),
            self.damage_out_exclusion_rules.len(),
        )
    }
}

impl AnalysisSettings {
    pub fn combatlog_file(&self) -> &Path {
        Path::new(&self.combatlog_file)
    }

    /// The four rule sets, lifted out of the settings.
    pub fn rule_sets(&self) -> RuleSets {
        RuleSets {
            version: RULES_FILE_VERSION,
            combat_name_rules: self.combat_name_rules.clone(),
            indirect_source_grouping_revers_rules: self
                .indirect_source_grouping_revers_rules
                .clone(),
            custom_group_rules: self.custom_group_rules.clone(),
            damage_out_exclusion_rules: self.damage_out_exclusion_rules.clone(),
        }
    }

    /// Put a set of rules in place of the ones held now.
    pub fn set_rule_sets(&mut self, sets: RuleSets) {
        self.combat_name_rules = sets.combat_name_rules;
        self.indirect_source_grouping_revers_rules = sets.indirect_source_grouping_revers_rules;
        self.custom_group_rules = sets.custom_group_rules;
        self.damage_out_exclusion_rules = sets.damage_out_exclusion_rules;
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
    /// Which ends of the name this method holds the pattern against.
    const fn anchors(self) -> (bool, bool) {
        match self {
            MatchMethod::Equals => (true, true),
            MatchMethod::StartsWith => (true, false),
            MatchMethod::EndsWith => (false, true),
            MatchMethod::Contains => (false, false),
        }
    }

    fn check_match(&self, expression: &str, value: &str) -> bool {
        // The plain path for a pattern with no wildcards in it — which is every
        // rule written before they existed, and most written after. Asked once
        // per condition per record, so the difference is worth keeping.
        if !has_wildcards(expression) {
            return match self {
                MatchMethod::Equals => value == expression,
                MatchMethod::StartsWith => value.starts_with(expression),
                MatchMethod::EndsWith => value.ends_with(expression),
                MatchMethod::Contains => value.contains(expression),
            };
        }
        let (start, end) = self.anchors();
        wildcard_matches_anchored(expression, value, start, end)
    }

    /// How precisely `expression` pins down `value` under this method, or
    /// `None` when it does not match it. See [`Specificity`].
    fn specificity(&self, expression: &str, value: &str) -> Option<Specificity> {
        if !self.check_match(expression, value) {
            return None;
        }
        // What the pattern actually spells out. A wildcard spells nothing: `?`
        // fixes a position without saying what is in it, and a run does not
        // even do that.
        let literal_characters = expression
            .chars()
            .filter(|c| !matches!(c, '*' | '%' | '?'))
            .count();
        let (start, end) = self.anchors();
        Some(Specificity {
            literal_characters,
            // Held at both ends, or held at one and spelling out the whole name
            // anyway.
            covers_the_whole_name: (start && end)
                || (!has_wildcards(expression)
                    && expression.chars().count() == value.chars().count()),
            // Only an exact, literal text admits nothing but the name it names.
            admits_nothing_else: matches!(self, MatchMethod::Equals) && !has_wildcards(expression),
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
        }
    }

    /// What the method does, in the words shown beside the picker.
    pub const fn explanation(self) -> &'static str {
        match self {
            MatchMethod::Equals => "the name is exactly this text",
            MatchMethod::StartsWith => "the name begins with this text",
            MatchMethod::EndsWith => "the name ends with this text",
            MatchMethod::Contains => "this text appears anywhere in the name",
        }
    }
}

/// Whether `value` matches `pattern`, with the pattern held at one end, both
/// ends, or neither.
///
/// `*` and `%` each stand for any run of characters (including none) and `?`
/// for exactly one. Both spellings of the run mean the same thing on purpose: a
/// player who knows file patterns writes `*Cannons*` and one who knows SQL
/// writes `%Cannons%`, and neither should have to find out which of the two
/// this program happens to take.
///
/// The anchors are what the four match methods are: `Equals` holds both ends,
/// `Starts with` the front, `Ends with` the back, `Contains` neither. So the
/// method says *where* the pattern sits and the pattern itself says what is in
/// it, and the two do not overlap. A pattern with no wildcard in it behaves
/// exactly as its method always did.
///
/// There is no escape character. Measured on a 138 MB log: no entity, pet,
/// target or ability name contains `*`, `%` or `?` — the game does not use
/// them — so there is nothing to escape and an escape rule would be one more
/// thing to explain for no case that occurs.
///
/// Case matters, as it does everywhere else in the rules.
///
/// No allocation, and linear in the length of the name: this is asked once per
/// condition per record, millions of times over a log, so it walks the two
/// strings in place. `run` remembers the last `*` and what it had consumed, and
/// a run that turns out too short is handed one more character rather than the
/// whole pattern being retried from the start.
pub fn wildcard_matches_anchored(
    pattern: &str,
    value: &str,
    anchor_start: bool,
    anchor_end: bool,
) -> bool {
    fn is_any_run(c: char) -> bool {
        c == '*' || c == '%'
    }

    let (mut p, mut v) = (pattern, value);
    // An unanchored front is a leading run: the pattern may start anywhere.
    let mut run: Option<(&str, &str)> = if anchor_start {
        None
    } else {
        Some((pattern, value))
    };

    loop {
        // An unanchored back is a trailing run: once the pattern is spent, what
        // is left of the name does not matter.
        if p.is_empty() && !anchor_end {
            return true;
        }
        let Some(vc) = v.chars().next() else {
            // The name is used up; what is left of the pattern may only be runs.
            return p.chars().all(is_any_run);
        };

        match p.chars().next() {
            Some(c) if is_any_run(c) => {
                p = &p[c.len_utf8()..];
                run = Some((p, v));
            }
            Some(c @ '?') => {
                p = &p[c.len_utf8()..];
                v = &v[vc.len_utf8()..];
            }
            Some(c) if c == vc => {
                p = &p[c.len_utf8()..];
                v = &v[vc.len_utf8()..];
            }
            // Nothing fits here: give the last run one more character and carry
            // on from just past it. With no run behind us there is nowhere to
            // go back to.
            _ => match run {
                Some((after_run, from)) => {
                    let skip = from.chars().next().map(char::len_utf8).unwrap_or(0);
                    if skip == 0 {
                        return false;
                    }
                    v = &from[skip..];
                    p = after_run;
                    run = Some((after_run, v));
                }
                None => return false,
            },
        }
    }
}

/// Whether `pattern` holds anything the matcher would read as a wildcard.
///
/// The four methods take the plain path when it does not, which is every rule
/// written before wildcards existed and most written after: `starts_with` on a
/// `&str` beats walking it a character at a time, and this is asked once per
/// condition per record.
fn has_wildcards(pattern: &str) -> bool {
    pattern.contains(['*', '%', '?'])
}

/// Guards on the rules file's format, for the next release rather than for
/// this one.
///
/// The file is read with unknown keys ignored, which is what makes a file
/// written by an older build still open — and also what makes a *renamed* field
/// vanish without a word, read as "the player never set that". So two things
/// are pinned here: the shape the current build writes, and the example file
/// published with the program. Between them, a change to the model cannot reach
/// a release without somebody deciding what to do about the files already out
/// there.
#[cfg(test)]
mod file_format_tests {
    use super::*;

    /// Where the example set published with the program lives, found from the
    /// crate root so the test does not care what directory it is run from.
    fn published_rules_file() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("rules")
            .join("STO-CLARE_Rules.toml")
    }

    /// Every field and every variant the model has, so that renaming any one of
    /// them lands here.
    fn one_of_everything() -> RuleSets {
        let rule = |aspect, method| MatchRule {
            aspect,
            expression: "Quad*Cannons".to_string(),
            method,
            enabled: true,
        };
        RuleSets {
            version: RULES_FILE_VERSION,
            combat_name_rules: vec![CombatNameRule {
                name_rule: RulesGroup {
                    name: "Infected".to_string(),
                    rules: vec![rule(MatchAspect::SourceOrTargetName, MatchMethod::Equals)],
                    enabled: true,
                },
                additional_info_rules: vec![RulesGroup {
                    name: "Space".to_string(),
                    rules: vec![rule(
                        MatchAspect::SourceOrTargetUniqueName,
                        MatchMethod::StartsWith,
                    )],
                    enabled: false,
                }],
            }],
            indirect_source_grouping_revers_rules: vec![rule(
                MatchAspect::IndirectSourceName,
                MatchMethod::EndsWith,
            )],
            custom_group_rules: vec![RulesGroup {
                name: "Quad Cannons".to_string(),
                rules: vec![rule(
                    MatchAspect::IndirectUniqueSourceName,
                    MatchMethod::Contains,
                )],
                enabled: true,
            }],
            damage_out_exclusion_rules: vec![rule(
                MatchAspect::DamageOrHealName,
                MatchMethod::Equals,
            )],
        }
    }

    /// What a rules file looks like today, written out in full.
    ///
    /// This is a promise to every file already on a player's disk, not a
    /// snapshot of the code: a key here is a key some rules file out there
    /// spells that way. If this test fails, the model changed shape, and the
    /// question to answer before it can be made green again is what happens to
    /// the files that are already written:
    ///
    /// - a field **added** with `#[serde(default)]`: older files still read, so
    ///   update the text below and leave `RULES_FILE_VERSION` alone;
    /// - a field **renamed or removed**, or a variant renamed: older files stop
    ///   carrying that setting *in silence*. Raise `RULES_FILE_VERSION`, read
    ///   the old spelling too, and convert the published file.
    ///
    /// Never re-paste the new output without answering that.
    #[test]
    fn the_shape_of_the_rules_file_is_pinned() {
        let written = toml::to_string_pretty(&one_of_everything()).unwrap();
        assert_eq!(
            EVERY_FIELD_OF_A_RULES_FILE, written,
            "the rules file's shape changed — see this test's notes before \
             updating it, and decide what happens to files already written"
        );
        assert_eq!(
            one_of_everything(),
            toml::from_str::<RuleSets>(EVERY_FIELD_OF_A_RULES_FILE).unwrap(),
            "and it has to read back as what it was"
        );
    }

    const EVERY_FIELD_OF_A_RULES_FILE: &str = r#"version = 1

[[combat_name_rules]]

[combat_name_rules.name_rule]
name = "Infected"
enabled = true

[[combat_name_rules.name_rule.rules]]
aspect = "SourceOrTargetName"
expression = "Quad*Cannons"
method = "Equals"
enabled = true

[[combat_name_rules.additional_info_rules]]
name = "Space"
enabled = false

[[combat_name_rules.additional_info_rules.rules]]
aspect = "SourceOrTargetUniqueName"
expression = "Quad*Cannons"
method = "StartsWith"
enabled = true

[[indirect_source_grouping_revers_rules]]
aspect = "IndirectSourceName"
expression = "Quad*Cannons"
method = "EndsWith"
enabled = true

[[custom_group_rules]]
name = "Quad Cannons"
enabled = true

[[custom_group_rules.rules]]
aspect = "IndirectUniqueSourceName"
expression = "Quad*Cannons"
method = "Contains"
enabled = true

[[damage_out_exclusion_rules]]
aspect = "DamageOrHealName"
expression = "Quad*Cannons"
method = "Equals"
enabled = true
"#;

    /// The example set published with the program is read by the build that
    /// publishes it. It is the file the manual tells a player to Import, and
    /// the one place where a model change could ship as a file nobody can use —
    /// nothing else in the suite ever opens it.
    #[test]
    fn the_published_rules_file_still_reads() {
        let path = published_rules_file();
        let sets = RuleSets::read(&path)
            .unwrap_or_else(|e| panic!("the published rules file must read: {e}"));

        assert_eq!(RULES_FILE_VERSION, sets.version);
        assert!(
            !sets.custom_group_rules.is_empty(),
            "it is published for its grouping rules"
        );
        // A rule that parsed but lost its conditions would still count as
        // "read", and would group nothing.
        for group in &sets.custom_group_rules {
            assert!(!group.name.is_empty(), "every group is named");
            assert!(
                !group.rules.is_empty(),
                "{} has no conditions left",
                group.name
            );
            for rule in &group.rules {
                assert!(
                    !rule.expression.is_empty(),
                    "{} has a condition matching nothing",
                    group.name
                );
            }
        }
        for rule in sets
            .damage_out_exclusion_rules
            .iter()
            .chain(&sets.indirect_source_grouping_revers_rules)
        {
            assert!(!rule.expression.is_empty(), "a condition matching nothing");
        }
    }

    /// Written and read back through a real file, not just through the parser:
    /// that is the path both the config directory and Export/Import take.
    #[test]
    fn everything_the_model_holds_survives_a_file() {
        let dir = std::env::temp_dir().join("cla-rules-shape");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("STO-CLARE_Rules.toml");

        let sets = one_of_everything();
        sets.write(&path).unwrap();
        assert_eq!(sets, RuleSets::read(&path).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
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
                > fit(MatchMethod::Contains, "*", ability)
        );
        assert!(
            fit(MatchMethod::Contains, "Phaser", ability)
                > fit(MatchMethod::Contains, "%", ability)
        );
    }

    #[test]
    fn a_wildcard_is_worth_its_literal_characters() {
        let ability = "Quad Disruptor Cannons - Rapid Fire III";
        assert!(
            fit(MatchMethod::Contains, "Quad*Cannons", ability)
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
            fit(MatchMethod::Equals, "Mk ?II", "Mk XII"),
            fit(MatchMethod::Equals, "Mk *II", "Mk XII"),
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

    /// Whether a pattern matches a name under one of the four methods.
    fn hit(method: MatchMethod, pattern: &str, value: &str) -> bool {
        method.check_match(pattern, value)
    }

    /// The two spellings of "any run of characters" are the same wildcard, so a
    /// player who reaches for either gets the same answer.
    #[test]
    fn star_and_percent_mean_the_same_thing() {
        for pattern in [
            "Quad*Cannons",
            "Quad%Cannons",
            "Quad*Cannons",
            "Quad%Cannons",
        ] {
            assert!(
                hit(
                    MatchMethod::StartsWith,
                    pattern,
                    "Quad Disruptor Cannons - Rapid Fire III"
                ),
                "{pattern} should have matched"
            );
        }
    }

    /// The method says where the pattern is held and the pattern says what is
    /// in it. The two are separate, which is why wildcards did not need a
    /// method of their own.
    #[test]
    fn the_method_only_decides_where_the_pattern_is_held() {
        let ability = "Quad Disruptor Cannons - Rapid Fire III";

        assert!(hit(MatchMethod::StartsWith, "Quad*Cannons", ability));
        assert!(hit(MatchMethod::Contains, "Disruptor*Rapid", ability));
        assert!(hit(MatchMethod::EndsWith, "Rapid*III", ability));
        assert!(hit(MatchMethod::Equals, "Quad*III", ability));

        // Held at the front, so a pattern that fits further in does not match.
        assert!(!hit(MatchMethod::StartsWith, "Disruptor*Rapid", ability));
        // Held at both ends, so it has to reach the end of the name.
        assert!(!hit(MatchMethod::Equals, "Quad*Rapid", ability));
    }

    /// Every rule written before wildcards existed has to go on meaning what it
    /// meant. Measured on the live settings: none of the 111 conditions there
    /// contains `*`, `%` or `?`, so this covers all of them.
    #[test]
    fn a_pattern_without_wildcards_behaves_as_its_method_always_did() {
        let ability = "Quad Disruptor Cannons";

        assert!(hit(MatchMethod::Equals, ability, ability));
        assert!(!hit(MatchMethod::Equals, "Quad", ability));
        assert!(hit(MatchMethod::StartsWith, "Quad", ability));
        assert!(!hit(MatchMethod::StartsWith, "Cannons", ability));
        assert!(hit(MatchMethod::EndsWith, "Cannons", ability));
        assert!(!hit(MatchMethod::EndsWith, "Quad", ability));
        assert!(hit(MatchMethod::Contains, "Disruptor", ability));
        assert!(!hit(MatchMethod::Contains, "Phaser", ability));
    }

    #[test]
    fn a_run_matches_nothing_at_all() {
        assert!(hit(MatchMethod::Equals, "Quad*Cannons", "QuadCannons"));
        assert!(hit(MatchMethod::Equals, "Quad*", "Quad"));
    }

    #[test]
    fn a_question_mark_stands_for_exactly_one_character() {
        assert!(hit(MatchMethod::Equals, "Mk ?II", "Mk XII"));
        assert!(!hit(MatchMethod::Equals, "Mk ?II", "Mk II"));
        assert!(!hit(MatchMethod::Equals, "Mk ?II", "Mk XIII"));
    }

    #[test]
    fn a_pattern_that_does_not_fit_is_refused() {
        assert!(!hit(
            MatchMethod::Contains,
            "Phaser*",
            "Quad Disruptor Cannons"
        ));
        assert!(!hit(
            MatchMethod::Contains,
            "Quad*Torpedo",
            "Quad Disruptor Cannons"
        ));
    }

    /// The run has to be able to give up ground it took too eagerly: the first
    /// `Cannons` here is not the one that lets the rest of the pattern fit.
    #[test]
    fn a_run_gives_back_what_it_took_too_early() {
        assert!(hit(
            MatchMethod::Equals,
            "*Cannons*III",
            "Cannons Cannons - Rapid Fire III"
        ));
        assert!(hit(MatchMethod::Equals, "*a*b", "aaab"));
    }

    #[test]
    fn an_empty_pattern_behaves_like_an_empty_text() {
        assert!(hit(MatchMethod::Equals, "", ""));
        assert!(!hit(MatchMethod::Equals, "", "Quad Cannons"));
        assert!(hit(MatchMethod::Equals, "*", ""));
        assert!(hit(MatchMethod::Contains, "", "Quad Cannons"));
    }

    /// Case is significant, as it is for every other kind of pattern.
    #[test]
    fn case_matters_as_it_does_elsewhere() {
        assert!(!hit(
            MatchMethod::Contains,
            "cannons*",
            "Quad Disruptor Cannons"
        ));
    }

    /// Names are not ASCII-only in principle, and the matcher walks a `&str` by
    /// byte offsets, so a multi-byte character must not split one.
    #[test]
    fn a_multi_byte_name_is_walked_safely() {
        assert!(hit(MatchMethod::Contains, "Ω*Ω", "aΩbΩc"));
        assert!(hit(MatchMethod::Equals, "?Ω?", "aΩb"));
        assert!(!hit(MatchMethod::Equals, "?Ω?", "aΩbc"));
    }

    /// The plain path and the matcher are two implementations of the same
    /// question, and a pattern with no wildcards in it goes down the first.
    /// They have to agree, or which one a pattern happens to take would change
    /// the answer — and the only patterns that exist today take the plain one.
    #[test]
    fn the_plain_path_and_the_matcher_agree_wherever_both_apply() {
        let names = [
            "Quad Disruptor Cannons",
            "Quad Disruptor Cannons - Rapid Fire III",
            "Terran Task Force Phaser Beam Array",
            "",
            "Quad",
            "Cannons",
        ];
        let patterns = [
            "Quad",
            "Cannons",
            "Quad Disruptor Cannons",
            "",
            "Phaser",
            "q",
        ];

        for method in [
            MatchMethod::Equals,
            MatchMethod::StartsWith,
            MatchMethod::EndsWith,
            MatchMethod::Contains,
        ] {
            let (start, end) = method.anchors();
            for pattern in patterns {
                for name in names {
                    let plain = match method {
                        MatchMethod::Equals => name == pattern,
                        MatchMethod::StartsWith => name.starts_with(pattern),
                        MatchMethod::EndsWith => name.ends_with(pattern),
                        MatchMethod::Contains => name.contains(pattern),
                    };
                    let matcher = wildcard_matches_anchored(pattern, name, start, end);
                    assert_eq!(
                        plain, matcher,
                        "{method:?} {pattern:?} against {name:?}: the plain path says \
                         {plain} and the matcher says {matcher}"
                    );
                }
            }
        }
    }

    /// Reached through a rule, which is how the analyzer asks.
    #[test]
    fn a_rule_carries_the_pattern_to_the_matcher() {
        let rule = MatchRule {
            aspect: MatchAspect::DamageOrHealName,
            expression: "Quad*Cannons".to_string(),
            method: MatchMethod::StartsWith,
            enabled: true,
        };
        assert!(rule.matches_damage_or_heal_name("Quad Disruptor Cannons - Rapid Fire III"));
        assert!(rule.matches_damage_or_heal_name("Quad Phaser Cannons"));
        assert!(!rule.matches_damage_or_heal_name("Terran Task Force Phaser Beam Array"));
    }

    /// A settings file written before wildcards existed still reads, and its
    /// methods still mean what they meant.
    #[test]
    fn an_older_settings_file_still_reads_back() {
        let stored = r#"{"aspect":"DamageOrHealName","expression":"Quad","method":"Contains","enabled":true}"#;
        let rule: MatchRule = serde_json::from_str(stored).unwrap();
        assert_eq!(MatchMethod::Contains, rule.method);
        assert!(rule.matches_damage_or_heal_name("Quad Disruptor Cannons"));
    }
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
