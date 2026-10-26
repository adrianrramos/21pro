//! Total-dependent 4–8 deck H17 strategy with DAS and late surrender.
//! Reference: <https://wizardofodds.com/games/blackjack/strategy/4-decks/>
//! The H17 image (not the page's S17 text transcription) is authoritative here.
use crate::model::{Action, HandKind, Situation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recommendation {
    pub action: Action,
    pub explanation: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexComparison {
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexPlay {
    pub ruleset: &'static str,
    pub kind: HandKind,
    pub value: u8,
    pub dealer: u8,
    pub comparison: IndexComparison,
    pub threshold: f32,
    pub action: Action,
    pub das_only: bool,
    pub surrender_only: bool,
    pub insurance: bool,
}

impl IndexPlay {
    pub const fn matches(self, true_count: f32) -> bool {
        match self.comparison {
            IndexComparison::Less => true_count < self.threshold,
            IndexComparison::LessOrEqual => true_count <= self.threshold,
            IndexComparison::Greater => true_count > self.threshold,
            IndexComparison::GreaterOrEqual => true_count >= self.threshold,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrategySource {
    BasicStrategy,
    CoreIndex,
}

pub const fn core_index_plays() -> &'static [IndexPlay] {
    &CORE_INDEX_PLAYS
}

/// Applies legal local indexes before the ordinary chart, preserving surrender
/// and split precedence. Inverse rows explicitly encode departures from the chart.
pub fn graded_action(
    situation: Situation,
    true_count: Option<f32>,
    indexes: &[IndexPlay],
) -> (Recommendation, StrategySource) {
    let basic = recommendation(situation);
    let Some(true_count) = true_count else {
        return (basic, StrategySource::BasicStrategy);
    };
    if situation.split_aces {
        return (basic, StrategySource::BasicStrategy);
    }
    let priority = |action| match action {
        Action::Surrender => 4,
        Action::Split => 3,
        Action::Double => 2,
        Action::Hit | Action::Stand => 1,
        Action::Insure | Action::DeclineInsurance => 0,
    };
    let mut selected = None;
    let mut selected_priority = if matches!(basic.action, Action::Surrender | Action::Split) {
        priority(basic.action)
    } else {
        0
    };
    for &index in indexes {
        let hand_matches = (index.kind == situation.kind && index.value == situation.value)
            || (index.kind == HandKind::Hard
                && situation.kind == HandKind::Pair
                && situation.value != 11
                && index.value == situation.value * 2);
        if index.ruleset != crate::model::RULESET_ID
            || !hand_matches
            || index.dealer != situation.dealer
            || (index.das_only && !situation.can_double)
            || (index.surrender_only && !situation.can_surrender)
            || !index.matches(true_count)
            || !situation.allows(index.action)
        {
            continue;
        }
        // A Fab 4 inverse hit replaces a legal chart surrender below its index.
        let index_priority = if index.surrender_only {
            priority(Action::Surrender)
        } else {
            priority(index.action)
        };
        if index_priority >= selected_priority {
            selected = Some(index);
            selected_priority = index_priority;
        }
    }
    selected.map_or((basic, StrategySource::BasicStrategy), |index| {
        (
            Recommendation {
                action: index.action,
                explanation: "Core index play applies at this true count.",
            },
            StrategySource::CoreIndex,
        )
    })
}

const CORE_INDEX_PLAYS: [IndexPlay; 22] = [
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Insurance,
        value: 0,
        dealer: 11,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 3.0,
        action: Action::Insure,
        das_only: false,
        surrender_only: false,
        insurance: true,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 16,
        dealer: 10,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 0.0,
        action: Action::Stand,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 15,
        dealer: 10,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 4.0,
        action: Action::Stand,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 10,
        dealer: 10,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 4.0,
        action: Action::Double,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 12,
        dealer: 3,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 2.0,
        action: Action::Stand,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 12,
        dealer: 2,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 3.0,
        action: Action::Stand,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 11,
        dealer: 11,
        comparison: IndexComparison::Less,
        threshold: -1.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 9,
        dealer: 2,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 1.0,
        action: Action::Double,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 10,
        dealer: 11,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 3.0,
        action: Action::Double,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 9,
        dealer: 7,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 3.0,
        action: Action::Double,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 16,
        dealer: 9,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 5.0,
        action: Action::Stand,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 13,
        dealer: 2,
        comparison: IndexComparison::Less,
        threshold: -1.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 12,
        dealer: 4,
        comparison: IndexComparison::Less,
        threshold: 0.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 12,
        dealer: 5,
        comparison: IndexComparison::Less,
        threshold: -2.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 12,
        dealer: 6,
        comparison: IndexComparison::Less,
        threshold: -3.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 13,
        dealer: 3,
        comparison: IndexComparison::Less,
        threshold: -2.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Pair,
        value: 10,
        dealer: 5,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 5.0,
        action: Action::Split,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Pair,
        value: 10,
        dealer: 6,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 4.0,
        action: Action::Split,
        das_only: false,
        surrender_only: false,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 15,
        dealer: 9,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 2.0,
        action: Action::Surrender,
        das_only: false,
        surrender_only: true,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 15,
        dealer: 11,
        comparison: IndexComparison::Less,
        threshold: -1.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: true,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 14,
        dealer: 10,
        comparison: IndexComparison::GreaterOrEqual,
        threshold: 3.0,
        action: Action::Surrender,
        das_only: false,
        surrender_only: true,
        insurance: false,
    },
    IndexPlay {
        ruleset: crate::model::RULESET_ID,
        kind: HandKind::Hard,
        value: 15,
        dealer: 10,
        comparison: IndexComparison::Less,
        threshold: 0.0,
        action: Action::Hit,
        das_only: false,
        surrender_only: true,
        insurance: false,
    },
];

pub fn recommendation(s: Situation) -> Recommendation {
    use Action::*;
    let result = |action, explanation| Recommendation {
        action,
        explanation,
    };
    if s.kind == HandKind::Insurance {
        return result(
            DeclineInsurance,
            "Never insure or take even money: the side bet has negative expectation without counting cards.",
        );
    }
    if s.split_aces {
        return if s.can_split {
            result(
                Split,
                "Resplit aces while capacity remains; each split ace receives only one card.",
            )
        } else {
            result(
                Stand,
                "The split limit is reached. Split aces cannot hit or double, so stand.",
            )
        };
    }
    // Pair 8,8 is a split against 9/10, but a surrender against A under H17.
    let surrender = match s.kind {
        HandKind::Hard => matches!((s.value, s.dealer), (15, 10 | 11) | (16, 9..=11) | (17, 11)),
        HandKind::Pair => s.value == 8 && s.dealer == 11,
        _ => false,
    };
    if s.can_surrender && surrender {
        return result(
            Surrender,
            "Late surrender loses half a bet instead of playing this unfavorable H17 matchup; dealer blackjack still takes the original bet.",
        );
    }
    if s.kind == HandKind::Pair && s.can_split {
        let split = match s.value {
            2 | 3 | 7 => (2..=7).contains(&s.dealer),
            4 => matches!(s.dealer, 5 | 6),
            6 => (2..=6).contains(&s.dealer),
            8 | 11 => true,
            9 => matches!(s.dealer, 2..=6 | 8 | 9),
            _ => false,
        };
        if split {
            return result(
                Split,
                "Splitting is best in this matchup with double after split allowed; split 21 pays as ordinary 21.",
            );
        }
    }
    let soft = s.kind == HandKind::Soft || (s.kind == HandKind::Pair && s.value == 11);
    let total = if s.kind == HandKind::Pair {
        if s.value == 11 { 12 } else { s.value * 2 }
    } else {
        s.value
    };
    let (double, stand) = if soft {
        let double = match total {
            13 | 14 => matches!(s.dealer, 5 | 6),
            15 | 16 => (4..=6).contains(&s.dealer),
            17 => (3..=6).contains(&s.dealer),
            18 => (2..=6).contains(&s.dealer),
            19 => s.dealer == 6,
            _ => false,
        };
        (double, total >= 19 || (total == 18 && s.dealer <= 8))
    } else {
        let double = match total {
            9 => (3..=6).contains(&s.dealer),
            10 => (2..=9).contains(&s.dealer),
            11 => true,
            _ => false,
        };
        (
            double,
            total >= 17
                || (total >= 13 && s.dealer <= 6)
                || (total == 12 && (4..=6).contains(&s.dealer)),
        )
    };
    if double && s.can_double {
        result(
            Double,
            "Double for one final card: this favorable H17 matchup benefits from an extra wager.",
        )
    } else if stand {
        result(
            Stand,
            if double {
                "Doubling is preferred, but unavailable here. The chart's fallback for this soft hand is stand."
            } else {
                "Stand: risking another card is worse than keeping this total against this upcard."
            },
        )
    } else {
        result(
            Hit,
            if double {
                "Doubling is preferred, but unavailable after a hit. Take another card instead."
            } else {
                "Hit: improving this total is worth the risk against this dealer upcard."
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently transcribed rows of the published H17 chart, columns 2..A.
    // D = double/hit; d = double/stand; R = surrender/hit;
    // r = surrender/stand; Q = surrender/split. DAS cells resolve to split.
    #[test]
    fn published_h17_chart_and_legal_fallbacks() {
        let hard = [
            (4, "HHHHHHHHHH"),
            (5, "HHHHHHHHHH"),
            (6, "HHHHHHHHHH"),
            (7, "HHHHHHHHHH"),
            (8, "HHHHHHHHHH"),
            (9, "HDDDDHHHHH"),
            (10, "DDDDDDDDHH"),
            (11, "DDDDDDDDDD"),
            (12, "HHSSSHHHHH"),
            (13, "SSSSSHHHHH"),
            (14, "SSSSSHHHHH"),
            (15, "SSSSSHHHRR"),
            (16, "SSSSSHHRRR"),
            (17, "SSSSSSSSSr"),
            (18, "SSSSSSSSSS"),
            (19, "SSSSSSSSSS"),
            (20, "SSSSSSSSSS"),
        ];
        let soft = [
            (13, "HHHDDHHHHH"),
            (14, "HHHDDHHHHH"),
            (15, "HHDDDHHHHH"),
            (16, "HHDDDHHHHH"),
            (17, "HDDDDHHHHH"),
            (18, "dddddSSHHH"),
            (19, "SSSSdSSSSS"),
            (20, "SSSSSSSSSS"),
        ];
        let pairs = [
            (2, "PPPPPPHHHH"),
            (3, "PPPPPPHHHH"),
            (4, "HHHPPHHHHH"),
            (5, "DDDDDDDDHH"),
            (6, "PPPPPHHHHH"),
            (7, "PPPPPPHHHH"),
            (8, "PPPPPPPPPQ"),
            (9, "PPPPPSPPSS"),
            (10, "SSSSSSSSSS"),
            (11, "PPPPPPPPPP"),
        ];
        for (kind, rows) in [
            (HandKind::Hard, hard.as_slice()),
            (HandKind::Soft, soft.as_slice()),
            (HandKind::Pair, pairs.as_slice()),
        ] {
            for &(value, row) in rows {
                for (column, code) in row.bytes().enumerate() {
                    for can_double in [false, true] {
                        for can_surrender in [false, true] {
                            let s = Situation {
                                kind,
                                value,
                                dealer: column as u8 + 2,
                                can_double,
                                can_split: kind == HandKind::Pair,
                                can_surrender,
                                split_aces: false,
                            };
                            let expected = match code {
                                b'H' => Action::Hit,
                                b'S' => Action::Stand,
                                b'P' => Action::Split,
                                b'D' if can_double => Action::Double,
                                b'D' => Action::Hit,
                                b'd' if can_double => Action::Double,
                                b'd' => Action::Stand,
                                b'R' | b'r' | b'Q' if can_surrender => Action::Surrender,
                                b'R' => Action::Hit,
                                b'r' => Action::Stand,
                                b'Q' => Action::Split,
                                _ => panic!("invalid chart cell"),
                            };
                            assert_eq!(recommendation(s).action, expected, "{s:?}");
                            assert!(s.allows(recommendation(s).action));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn published_h17_indexes_grade_both_sides_of_every_boundary() {
        use Action::*;
        use HandKind::*;
        use IndexComparison::{GreaterOrEqual, Less};
        // Expected actions below / at / above the published H17 crossover.
        let expected = [
            (
                Insurance,
                0,
                11,
                GreaterOrEqual,
                3.0,
                Insure,
                false,
                [DeclineInsurance, Insure, Insure],
            ),
            (
                Hard,
                16,
                10,
                GreaterOrEqual,
                0.0,
                Stand,
                false,
                [Hit, Stand, Stand],
            ),
            (
                Hard,
                15,
                10,
                GreaterOrEqual,
                4.0,
                Stand,
                false,
                [Hit, Stand, Stand],
            ),
            (
                Pair,
                10,
                5,
                GreaterOrEqual,
                5.0,
                Split,
                false,
                [Stand, Split, Split],
            ),
            (
                Pair,
                10,
                6,
                GreaterOrEqual,
                4.0,
                Split,
                false,
                [Stand, Split, Split],
            ),
            (
                Hard,
                10,
                10,
                GreaterOrEqual,
                4.0,
                Double,
                false,
                [Hit, Double, Double],
            ),
            (
                Hard,
                12,
                3,
                GreaterOrEqual,
                2.0,
                Stand,
                false,
                [Hit, Stand, Stand],
            ),
            (
                Hard,
                12,
                2,
                GreaterOrEqual,
                3.0,
                Stand,
                false,
                [Hit, Stand, Stand],
            ),
            (Hard, 11, 11, Less, -1.0, Hit, false, [Hit, Double, Double]),
            (
                Hard,
                9,
                2,
                GreaterOrEqual,
                1.0,
                Double,
                false,
                [Hit, Double, Double],
            ),
            (
                Hard,
                10,
                11,
                GreaterOrEqual,
                3.0,
                Double,
                false,
                [Hit, Double, Double],
            ),
            (
                Hard,
                9,
                7,
                GreaterOrEqual,
                3.0,
                Double,
                false,
                [Hit, Double, Double],
            ),
            (
                Hard,
                16,
                9,
                GreaterOrEqual,
                5.0,
                Stand,
                false,
                [Hit, Stand, Stand],
            ),
            (Hard, 13, 2, Less, -1.0, Hit, false, [Hit, Stand, Stand]),
            (Hard, 12, 4, Less, 0.0, Hit, false, [Hit, Stand, Stand]),
            (Hard, 12, 5, Less, -2.0, Hit, false, [Hit, Stand, Stand]),
            (Hard, 12, 6, Less, -3.0, Hit, false, [Hit, Stand, Stand]),
            (Hard, 13, 3, Less, -2.0, Hit, false, [Hit, Stand, Stand]),
            (
                Hard,
                14,
                10,
                GreaterOrEqual,
                3.0,
                Surrender,
                true,
                [Hit, Surrender, Surrender],
            ),
            (
                Hard,
                15,
                9,
                GreaterOrEqual,
                2.0,
                Surrender,
                true,
                [Hit, Surrender, Surrender],
            ),
            (
                Hard,
                15,
                10,
                Less,
                0.0,
                Hit,
                true,
                [Hit, Surrender, Surrender],
            ),
            (
                Hard,
                15,
                11,
                Less,
                -1.0,
                Hit,
                true,
                [Hit, Surrender, Surrender],
            ),
        ];
        assert_eq!(core_index_plays().len(), expected.len());
        for (kind, value, dealer, comparison, threshold, action, surrender_only, actions) in
            expected
        {
            let play = core_index_plays()
                .iter()
                .find(|play| {
                    play.kind == kind
                        && play.value == value
                        && play.dealer == dealer
                        && play.surrender_only == surrender_only
                })
                .expect("published play is present");
            assert_eq!(play.comparison, comparison);
            assert_eq!(play.threshold, threshold);
            assert_eq!(play.action, action);
            assert_eq!(play.insurance, kind == Insurance);
            assert_eq!(play.ruleset, crate::model::RULESET_ID);
            let situation = Situation {
                kind,
                value,
                dealer,
                can_double: true,
                can_split: kind == Pair,
                can_surrender: surrender_only,
                split_aces: false,
            };
            for (count, expected_action) in [threshold - 0.01, threshold, threshold + 0.01]
                .into_iter()
                .zip(actions)
            {
                let (actual, source) = graded_action(situation, Some(count), core_index_plays());
                assert_eq!(actual.action, expected_action, "{play:?} at {count}");
                assert!(situation.allows(actual.action));
                assert_eq!(
                    source,
                    if play.matches(count) {
                        StrategySource::CoreIndex
                    } else {
                        StrategySource::BasicStrategy
                    },
                    "{play:?} at {count}"
                );
            }
        }
    }

    #[test]
    fn index_legality_surrender_and_split_precedence() {
        let mut s = Situation {
            kind: HandKind::Hard,
            value: 16,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: true,
            split_aces: false,
        };
        for count in [-8.0, 0.0, 8.0] {
            assert_eq!(
                graded_action(s, Some(count), core_index_plays()).0.action,
                Action::Surrender
            );
        }
        s.value = 15;
        assert_eq!(
            graded_action(s, Some(-0.01), core_index_plays()).0.action,
            Action::Hit
        );
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Surrender
        );
        s.can_surrender = false;
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Stand
        );
        s.value = 14;
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Hit
        );
        s.value = 10;
        s.can_double = false;
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Hit
        );
        s.kind = HandKind::Pair;
        s.value = 10;
        s.dealer = 6;
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Stand
        );
        s.can_split = true;
        assert_eq!(
            graded_action(s, Some(8.0), core_index_plays()).0.action,
            Action::Split
        );
        // Hard-total indexes also apply to pairs when the ordinary split is unavailable.
        s.value = 6;
        s.dealer = 4;
        assert_eq!(
            graded_action(s, Some(-1.0), core_index_plays()).0.action,
            Action::Split
        );
        s.can_split = false;
        assert_eq!(
            graded_action(s, Some(-1.0), core_index_plays()).0.action,
            Action::Hit
        );
        s.value = 5;
        s.dealer = 10;
        s.can_double = true;
        assert_eq!(
            graded_action(s, Some(4.0), core_index_plays()).0.action,
            Action::Double
        );
    }

    #[test]
    fn no_index_count_or_matching_play_preserves_ordinary_chart() {
        for kind in [
            HandKind::Hard,
            HandKind::Soft,
            HandKind::Pair,
            HandKind::Insurance,
        ] {
            for value in 2..=20 {
                for dealer in 2..=11 {
                    let s = Situation {
                        kind,
                        value,
                        dealer,
                        can_double: true,
                        can_split: kind == HandKind::Pair,
                        can_surrender: true,
                        split_aces: false,
                    };
                    assert_eq!(
                        graded_action(s, None, core_index_plays()),
                        (recommendation(s), StrategySource::BasicStrategy)
                    );
                    assert_eq!(
                        graded_action(s, Some(8.0), &[]),
                        (recommendation(s), StrategySource::BasicStrategy)
                    );
                }
            }
        }
    }

    #[test]
    fn index_comparators_keep_strict_and_inclusive_boundaries_distinct() {
        let play = core_index_plays()[0];
        for (comparison, expected) in [
            (IndexComparison::Less, [true, false, false]),
            (IndexComparison::LessOrEqual, [true, true, false]),
            (IndexComparison::Greater, [false, false, true]),
            (IndexComparison::GreaterOrEqual, [false, true, true]),
        ] {
            let play = IndexPlay { comparison, ..play };
            assert_eq!(
                [play.matches(2.99), play.matches(3.0), play.matches(3.01)],
                expected
            );
        }
    }

    #[test]
    fn split_limit_and_insurance_fallbacks() {
        let mut s = Situation {
            kind: HandKind::Pair,
            value: 8,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        };
        assert_eq!(recommendation(s).action, Action::Hit);
        s.value = 11;
        s.split_aces = true;
        s.can_double = false;
        assert_eq!(recommendation(s).action, Action::Stand);
        assert_eq!(
            graded_action(s, Some(-8.0), core_index_plays()).0.action,
            Action::Stand
        );
        s.can_split = true;
        assert_eq!(recommendation(s).action, Action::Split);
        assert_eq!(
            graded_action(s, Some(-8.0), core_index_plays()).0.action,
            Action::Split
        );
        s.kind = HandKind::Insurance;
        assert_eq!(recommendation(s).action, Action::DeclineInsurance);
    }
}
