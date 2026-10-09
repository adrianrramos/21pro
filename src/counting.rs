//! Hi-Lo practice mechanics, independent of the desktop renderer and learning schedule.
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Card, Rank, Suit};

pub const TRIAL_SIZE: usize = 52;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountingRecord {
    pub completed_at: i64,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TrialError {
    #[error("the trial has already started")]
    AlreadyStarted,
    #[error("the trial already has 52 revealed cards")]
    Complete,
}

#[derive(Debug, Clone)]
pub struct CountingTrial {
    shoe: Vec<Card>,
    revealed: Vec<Card>,
    running_count: i32,
}

impl CountingTrial {
    pub fn new(seed: u64) -> Self {
        let mut shoe = Vec::with_capacity(6 * Suit::ALL.len() * Rank::ALL.len());
        for _ in 0..6 {
            for suit in Suit::ALL {
                for rank in Rank::ALL {
                    shoe.push(Card { rank, suit });
                }
            }
        }
        let mut rng = SmallRng::seed_from_u64(seed);
        shoe.shuffle(&mut rng);
        Self {
            shoe,
            revealed: Vec::with_capacity(TRIAL_SIZE),
            running_count: 0,
        }
    }

    pub fn start(&mut self) -> Result<Card, TrialError> {
        if !self.revealed.is_empty() {
            return Err(TrialError::AlreadyStarted);
        }
        self.reveal_next()
    }

    pub fn next_card(&mut self) -> Result<Card, TrialError> {
        if self.revealed.is_empty() {
            return self.start();
        }
        self.reveal_next()
    }

    pub fn current_card(&self) -> Option<Card> {
        self.revealed.last().copied()
    }

    pub fn cards_seen(&self) -> usize {
        self.revealed.len()
    }

    pub fn is_complete(&self) -> bool {
        self.cards_seen() == TRIAL_SIZE
    }

    pub fn actual_count(&self) -> i32 {
        self.running_count
    }

    pub fn revealed_cards(&self) -> &[Card] {
        &self.revealed
    }

    fn reveal_next(&mut self) -> Result<Card, TrialError> {
        if self.is_complete() {
            return Err(TrialError::Complete);
        }
        let card = self.shoe.pop().ok_or(TrialError::Complete)?;
        self.running_count += i32::from(card_value(card.rank));
        self.revealed.push(card);
        Ok(card)
    }
}

pub const fn card_value(rank: Rank) -> i8 {
    match rank {
        Rank::Two | Rank::Three | Rank::Four | Rank::Five | Rank::Six => 1,
        Rank::Seven | Rank::Eight | Rank::Nine => 0,
        Rank::Ace | Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => -1,
    }
}

pub fn count_cards(cards: &[Card]) -> i32 {
    cards
        .iter()
        .map(|card| i32::from(card_value(card.rank)))
        .sum()
}

pub fn parse_submitted_count(input: &str) -> Option<i32> {
    input.trim().parse().ok()
}

pub fn sorted_history(history: &[CountingRecord]) -> Vec<CountingRecord> {
    let mut sorted = history.to_vec();
    sorted.sort_by_key(|record| (record.duration_ms, record.completed_at));
    sorted
}

/// Format persisted completion timestamps as UTC so history is unambiguous offline.
pub fn format_completed_at(timestamp: i64) -> String {
    let days = timestamp.div_euclid(86_400);
    let seconds = timestamp.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds / 3_600;
    let minute = seconds / 60 % 60;
    let second = seconds % 60;
    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02} UTC")
}

// Howard Hinnant's proleptic Gregorian civil-date conversion, using only the
// standard library and supporting every non-negative Unix timestamp.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_part = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_part + 2) / 5 + 1;
    let month = month_part + if month_part < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hi_lo_values_match_the_counting_system() {
        assert_eq!(card_value(Rank::Two), 1);
        assert_eq!(card_value(Rank::Six), 1);
        assert_eq!(card_value(Rank::Seven), 0);
        assert_eq!(card_value(Rank::Nine), 0);
        assert_eq!(card_value(Rank::Ten), -1);
        assert_eq!(card_value(Rank::Ace), -1);
    }

    #[test]
    fn submitted_count_accepts_signed_integers_and_rejects_other_text() {
        assert_eq!(parse_submitted_count("+3"), Some(3));
        assert_eq!(parse_submitted_count("-2"), Some(-2));
        assert_eq!(parse_submitted_count(" 0 "), Some(0));
        assert!(parse_submitted_count("3.5").is_none());
        assert!(parse_submitted_count("").is_none());
    }

    #[test]
    fn trial_reveals_exactly_52_cards_without_exceeding_six_copies() {
        let mut trial = CountingTrial::new(42);
        trial.start().unwrap();
        while !trial.is_complete() {
            trial.next_card().unwrap();
        }

        assert_eq!(trial.cards_seen(), 52);
        assert!(trial.next_card().is_err());
        for suit in Suit::ALL {
            for rank in Rank::ALL {
                let copies = trial
                    .revealed_cards()
                    .iter()
                    .filter(|card| card.suit == suit && card.rank == rank)
                    .count();
                assert!(copies <= 6);
            }
        }
    }

    #[test]
    fn trial_starts_with_one_card_and_rejects_double_start() {
        let mut trial = CountingTrial::new(7);
        assert!(trial.current_card().is_none());
        assert!(trial.start().is_ok());
        assert!(trial.current_card().is_some());
        assert_eq!(trial.cards_seen(), 1);
        assert!(matches!(trial.start(), Err(TrialError::AlreadyStarted)));
    }

    #[test]
    fn fixed_cards_produce_the_expected_running_count() {
        let cards = [
            Card {
                rank: Rank::Two,
                suit: Suit::Clubs,
            },
            Card {
                rank: Rank::Seven,
                suit: Suit::Diamonds,
            },
            Card {
                rank: Rank::King,
                suit: Suit::Hearts,
            },
            Card {
                rank: Rank::Six,
                suit: Suit::Spades,
            },
        ];
        assert_eq!(count_cards(&cards), 1);
    }

    #[test]
    fn history_is_sorted_by_shortest_duration_then_completion_time() {
        let history = [
            CountingRecord {
                completed_at: 30,
                duration_ms: 4_000,
            },
            CountingRecord {
                completed_at: 10,
                duration_ms: 2_000,
            },
            CountingRecord {
                completed_at: 20,
                duration_ms: 2_000,
            },
        ];
        let sorted = sorted_history(&history);
        assert_eq!(
            sorted
                .iter()
                .map(|record| record.completed_at)
                .collect::<Vec<_>>(),
            vec![10, 20, 30]
        );
    }

    #[test]
    fn timestamps_are_displayed_as_utc_dates() {
        assert_eq!(format_completed_at(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(
            format_completed_at(1_735_689_600),
            "2025-01-01 00:00:00 UTC"
        );
    }
}
