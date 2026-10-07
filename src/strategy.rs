//! Total-dependent 4–8 deck H17 strategy with DAS and late surrender.
//! Reference: <https://wizardofodds.com/games/blackjack/strategy/4-decks/>
//! The H17 image (not the page's S17 text transcription) is authoritative here.
use crate::model::{Action, HandKind, Situation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recommendation {
    pub action: Action,
    pub explanation: &'static str,
}

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
        s.can_split = true;
        assert_eq!(recommendation(s).action, Action::Split);
        s.kind = HandKind::Insurance;
        assert_eq!(recommendation(s).action, Action::DeclineInsurance);
    }
}
