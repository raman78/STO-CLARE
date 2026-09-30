//! Why a fight is on the map it is, at the level it is — or why not.
//!
//! The detection (`analyzer::detection`) reaches its answer in steps: which
//! entity gave the map away, which tier tables it checked and where each one
//! failed, how the ship-class vote went. Without them a fight the program could
//! not place shows only a dash, and there is no telling a map it has never heard
//! of from one whose Elite run lost a kill to a combat split. This turns the
//! steps into the text a fight's row in the combats list adds to its tooltip,
//! where the name leaves a question open ([`worth_explaining`]).

use crate::{
    analyzer::{CombatSummary, DetectionStep, Difficulty, TableKind, TableOutcome},
    helpers::number_formatting::NumberFormatter,
};

/// Whether the detection has anything to say that the fight's name does not.
/// A map recognized at a settled level, with no rival map and no vote
/// overturning the map's own checks, is fully said by its name; explaining it
/// only repeats that. What is left is every case the name leaves open.
pub fn worth_explaining(combat: &CombatSummary) -> bool {
    if combat.detection.is_empty() {
        return false;
    }
    let level_settled = combat.difficulty.and_then(|d| d.label()).is_some();
    !level_settled
        || combat.detection.iter().any(|step| match step {
            DetectionStep::NoAnchor | DetectionStep::AlsoMatched { .. } => true,
            DetectionStep::ShipClassVote { vote, from_tables } => {
                matches!((vote.winner(), from_tables),
                    (Some(winner), Some(tables)) if winner != *tables && *tables != Difficulty::Any)
            }
            _ => false,
        })
}

/// The explanation for one fight, one line per step, ending with the verdict on
/// the level.
pub fn explain(combat: &CombatSummary, formatter: &mut NumberFormatter) -> String {
    let mut lines = Vec::new();
    if combat.named_by_rule {
        lines.push("The name comes from one of your naming rules.".to_owned());
    }
    for step in combat.detection.iter() {
        lines.push(step_line(step, formatter));
    }
    if !matches!(
        combat.detection.first(),
        None | Some(DetectionStep::NoAnchor)
    ) {
        lines.push(match combat.difficulty.and_then(|d| d.label()) {
            Some(level) => format!("Level: {level}."),
            None => "Level: could not be told.".to_owned(),
        });
    }
    lines.join("\n")
}

fn step_line(step: &DetectionStep, formatter: &mut NumberFormatter) -> String {
    match step {
        DetectionStep::NoAnchor => "Not recognized: none of the ships or objects that give a \
                                    known map away were in this fight."
            .to_owned(),
        DetectionStep::Anchor { map, entities } => {
            format!("Recognized as {map} by: {}", entities.join(", "))
        }
        DetectionStep::AlsoMatched { map } => {
            format!("{map} matched as well, and was passed over.")
        }
        DetectionStep::Pinned(difficulty) => format!(
            "This map is only played at one level: {}.",
            level(*difficulty)
        ),
        DetectionStep::Table {
            tier,
            kind,
            outcome,
        } => {
            let test = match kind {
                TableKind::Deaths => "by kills",
                TableKind::Hull => "by hull",
                TableKind::HullAny => "by hull",
            };
            format!(
                "{} {test}: {}",
                level(*tier),
                outcome_text(outcome, formatter)
            )
        }
        DetectionStep::ShipClassVote { vote, from_tables } => {
            let tally = format!(
                "Ship-class vote: Elite {}, Advanced {}, Normal {}",
                vote.elite, vote.advanced, vote.normal
            );
            match vote.winner() {
                None if vote.elite + vote.advanced + vote.normal == 0 => {
                    "Ship-class vote: no ship that died could vote.".to_owned()
                }
                None => format!("{tally} — a tie, so no decision."),
                Some(winner) => match from_tables {
                    Some(tables) if *tables != winner && *tables != Difficulty::Any => format!(
                        "{tally} → {}, replacing {} from the map's own checks.",
                        level(winner),
                        level(*tables)
                    ),
                    _ => format!("{tally} → {}.", level(winner)),
                },
            }
        }
    }
}

fn outcome_text(outcome: &TableOutcome, formatter: &mut NumberFormatter) -> String {
    match outcome {
        TableOutcome::Matched => "yes".to_owned(),
        TableOutcome::Missing { entity } => format!("no — {entity} was not in the fight"),
        TableOutcome::Deaths {
            entity,
            expected,
            got,
        } => format!("no — {entity} was killed {got}×, expected {expected}×"),
        TableOutcome::Survived { entity } => {
            format!("no — {entity} was never killed, so its hull says nothing")
        }
        TableOutcome::LowHull {
            entity,
            needed,
            median,
        } => format!(
            "no — {entity} took {} hull damage, needs over {}",
            formatter.format_with_automated_suffixes(*median),
            formatter.format_with_automated_suffixes(*needed)
        ),
        TableOutcome::NonePresent => {
            "no — none of the ships it checks were in the fight".to_owned()
        }
    }
}

fn level(difficulty: Difficulty) -> &'static str {
    difficulty.label().unwrap_or("any level")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::ShipClassVote;

    fn summary(steps: Vec<DetectionStep>, difficulty: Option<Difficulty>) -> CombatSummary {
        CombatSummary {
            name: String::new(),
            identifier: String::new(),
            base_name: String::new(),
            category: None,
            environment: None,
            difficulty,
            detection: steps.into(),
            named_by_rule: false,
            solo: true,
            start: Default::default(),
            duration: Default::default(),
            players: Vec::new(),
            abilities: std::sync::Arc::from([]),
        }
    }

    /// The fight from the OSCR screenshot: both kill tables fail, the vote
    /// settles it. Every failure has to name its entity and both counts.
    #[test]
    fn failed_kill_tables_name_the_entity_and_both_counts() {
        let steps = vec![
            DetectionStep::Anchor {
                map: "[TFO] Infected: The Conduit".to_owned(),
                entities: vec!["Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss".to_owned()],
            },
            DetectionStep::Table {
                tier: Difficulty::Elite,
                kind: TableKind::Deaths,
                outcome: TableOutcome::Deaths {
                    entity: "Space_Borg_Battleship_Raidisode_Sibrian_Elite_Initial".to_owned(),
                    expected: 2,
                    got: 1,
                },
            },
            DetectionStep::ShipClassVote {
                vote: ShipClassVote {
                    normal: 0,
                    advanced: 0,
                    elite: 9,
                },
                from_tables: None,
            },
        ];
        let text = explain(
            &summary(steps, Some(Difficulty::Elite)),
            &mut NumberFormatter::new(),
        );
        assert_eq!(
            text,
            "Recognized as [TFO] Infected: The Conduit by: \
             Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss\n\
             Elite by kills: no — Space_Borg_Battleship_Raidisode_Sibrian_Elite_Initial was \
             killed 1×, expected 2×\n\
             Ship-class vote: Elite 9, Advanced 0, Normal 0 → Elite.\n\
             Level: Elite."
        );
    }

    #[test]
    fn unrecognized_fight_says_so_and_nothing_about_a_level() {
        let text = explain(
            &summary(vec![DetectionStep::NoAnchor], None),
            &mut NumberFormatter::new(),
        );
        assert!(text.starts_with("Not recognized"));
        assert!(!text.contains("Level"));
    }

    #[test]
    fn a_vote_that_overrides_the_tables_says_what_it_replaced() {
        let steps = vec![DetectionStep::ShipClassVote {
            vote: ShipClassVote {
                normal: 0,
                advanced: 1,
                elite: 4,
            },
            from_tables: Some(Difficulty::Advanced),
        }];
        let text = explain(
            &summary(steps, Some(Difficulty::Elite)),
            &mut NumberFormatter::new(),
        );
        assert!(text.contains("→ Elite, replacing Advanced from the map's own checks."));
    }

    #[test]
    fn an_unsettled_level_is_said_to_be_unsettled() {
        let steps = vec![DetectionStep::Anchor {
            map: "M".to_owned(),
            entities: vec!["E".to_owned()],
        }];
        let text = explain(
            &summary(steps, Some(Difficulty::Any)),
            &mut NumberFormatter::new(),
        );
        assert!(text.ends_with("Level: could not be told."));
    }

    fn recognized(difficulty: Option<Difficulty>, extra: Vec<DetectionStep>) -> CombatSummary {
        let mut steps = vec![DetectionStep::Anchor {
            map: "M".to_owned(),
            entities: vec!["E".to_owned()],
        }];
        steps.extend(extra);
        summary(steps, difficulty)
    }

    #[test]
    fn a_plainly_recognized_fight_is_not_explained() {
        let vote = DetectionStep::ShipClassVote {
            vote: ShipClassVote {
                normal: 0,
                advanced: 0,
                elite: 4,
            },
            from_tables: Some(Difficulty::Elite),
        };
        assert!(!worth_explaining(&recognized(
            Some(Difficulty::Elite),
            vec![vote]
        )));
        assert!(!worth_explaining(&recognized(
            Some(Difficulty::Normal),
            vec![DetectionStep::Pinned(Difficulty::Normal)]
        )));
    }

    #[test]
    fn every_open_question_is_explained() {
        // Not recognized.
        assert!(worth_explaining(&summary(
            vec![DetectionStep::NoAnchor],
            None
        )));
        // Recognized, level unsettled.
        assert!(worth_explaining(&recognized(Some(Difficulty::Any), vec![])));
        assert!(worth_explaining(&recognized(None, vec![])));
        // Another map matched too.
        assert!(worth_explaining(&recognized(
            Some(Difficulty::Elite),
            vec![DetectionStep::AlsoMatched {
                map: "N".to_owned()
            }]
        )));
        // The vote overturned the map's own checks.
        assert!(worth_explaining(&recognized(
            Some(Difficulty::Elite),
            vec![DetectionStep::ShipClassVote {
                vote: ShipClassVote {
                    normal: 0,
                    advanced: 1,
                    elite: 4,
                },
                from_tables: Some(Difficulty::Advanced),
            }]
        )));
    }

    /// A fight the detection never ran on (no steps) has nothing to explain.
    #[test]
    fn no_steps_nothing_to_explain() {
        assert!(!worth_explaining(&summary(vec![], None)));
    }
}
