//! Hi-Lo practice mechanics, independent of the desktop renderer and learning schedule.
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{Card, Rank, Suit};

/// Number of cards revealed by one card-counting trial.
pub const TRIAL_SIZE: usize = 52;

/// A completed correct card-counting trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountingRecord {
    /// Unix completion timestamp in seconds.
    pub completed_at: i64,
    /// Elapsed trial duration in milliseconds.
    pub duration_ms: u64,
}

/// Invalid state transition while revealing a counting trial.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TrialError {
    /// The trial was started more than once.
    #[error("the trial has already started")]
    AlreadyStarted,
    /// All 52 trial cards have already been revealed.
    #[error("the trial already has 52 revealed cards")]
    Complete,
}

/// A freshly shuffled six-deck shoe that reveals one card at a time.
#[derive(Debug, Clone)]
pub struct CountingTrial {
    shoe: Vec<Card>,
    revealed: Vec<Card>,
    running_count: i32,
}

impl CountingTrial {
    /// Creates a deterministic trial from a seed.
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

    /// Reveals card one and starts the trial.
    pub fn start(&mut self) -> Result<Card, TrialError> {
        if !self.revealed.is_empty() {
            return Err(TrialError::AlreadyStarted);
        }
        self.reveal_next()
    }

    /// Reveals the next card, or starts the trial if it has not started.
    pub fn next_card(&mut self) -> Result<Card, TrialError> {
        if self.revealed.is_empty() {
            return self.start();
        }
        self.reveal_next()
    }

    /// Returns the most recently revealed card.
    pub fn current_card(&self) -> Option<Card> {
        self.revealed.last().copied()
    }

    /// Returns the number of revealed cards.
    pub fn cards_seen(&self) -> usize {
        self.revealed.len()
    }

    /// Returns whether all cards in the trial have been revealed.
    pub fn is_complete(&self) -> bool {
        self.cards_seen() == TRIAL_SIZE
    }

    /// Returns the final Hi-Lo running count so far.
    pub fn actual_count(&self) -> i32 {
        self.running_count
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

/// Returns the Hi-Lo value for a card rank.
pub const fn card_value(rank: Rank) -> i8 {
    match rank {
        Rank::Two | Rank::Three | Rank::Four | Rank::Five | Rank::Six => 1,
        Rank::Seven | Rank::Eight | Rank::Nine => 0,
        Rank::Ace | Rank::Ten | Rank::Jack | Rank::Queen | Rank::King => -1,
    }
}

/// Parses a signed whole-number answer after trimming whitespace.
pub fn parse_submitted_count(input: &str) -> Option<i32> {
    input.trim().parse().ok()
}

/// Returns a copy sorted by shortest duration, then completion time.
pub fn sorted_history(history: &[CountingRecord]) -> Vec<CountingRecord> {
    let mut sorted = history.to_vec();
    sorted.sort_by_key(|record| (record.duration_ms, record.completed_at));
    sorted
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
                    .revealed
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
}
