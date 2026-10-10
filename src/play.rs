//! Renderer-independent bankroll, stake, settlement, and session-history logic.
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::game::{Game, GameError, Phase};
use crate::model::Action;

pub const FREE_PLAY_SCHEMA_VERSION: u32 = 1;
pub const DEFAULT_BANKROLL_CENTS: i64 = 100_000;
pub const MINIMUM_WAGER_CENTS: i64 = 500;
pub const CHIP_DENOMINATIONS: [i64; 4] = [500, 2_500, 10_000, 100_000];
const MAX_MONEY_CENTS: i64 = i64::MAX / 4;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayPoint {
    pub round: u32,
    pub bankroll_cents: i64,
    pub cumulative_net_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlayError {
    #[error("Free Play bankroll and stakes must use positive whole cents.")]
    InvalidMoney,
    #[error("Free Play bankroll is too large to represent safely.")]
    MoneyOverflow,
    #[error("A base wager must be a positive multiple of $5.")]
    InvalidWager,
    #[error("The wager exceeds available funds.")]
    InsufficientFunds,
    #[error("Finish or reset the current round before changing the wager.")]
    WagerLocked,
    #[error("No wager is ready to deal.")]
    NoWager,
    #[error("This action is not affordable.")]
    ActionUnaffordable,
    #[error("Invalid saved Free Play session: {0}")]
    InvalidSnapshot(String),
    #[error("Unsupported Free Play schema {found}; this application supports {supported}")]
    Version { found: u32, supported: u32 },
    #[error("{0}")]
    Game(#[from] GameError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaySession {
    pub schema_version: u32,
    pub starting_bankroll_cents: i64,
    pub available_cents: i64,
    pub committed_cents: i64,
    pub pending_wager_cents: i64,
    pub wager_chips: Vec<i64>,
    pub locked_wager_cents: Option<i64>,
    pub history: Vec<PlayPoint>,
    pub game: Game,
    pub round_settled: bool,
}

impl PlaySession {
    pub fn new(seed: u64) -> Self {
        let bankroll = DEFAULT_BANKROLL_CENTS;
        Self {
            schema_version: FREE_PLAY_SCHEMA_VERSION,
            starting_bankroll_cents: bankroll,
            available_cents: bankroll,
            committed_cents: 0,
            pending_wager_cents: 0,
            wager_chips: Vec::new(),
            locked_wager_cents: None,
            history: vec![PlayPoint {
                round: 0,
                bankroll_cents: bankroll,
                cumulative_net_cents: 0,
            }],
            game: Game::new(seed),
            round_settled: true,
        }
    }

    pub fn phase(&self) -> Phase {
        self.game.phase
    }

    pub fn remaining_cards(&self) -> usize {
        self.game.remaining_cards()
    }

    pub fn current_wager_cents(&self) -> i64 {
        self.locked_wager_cents.unwrap_or(self.pending_wager_cents)
    }

    pub fn cumulative_net_cents(&self) -> i64 {
        self.history
            .last()
            .map_or(0, |point| point.cumulative_net_cents)
    }

    pub fn add_chip(&mut self, cents: i64) -> Result<(), PlayError> {
        if !matches!(self.phase(), Phase::Ready | Phase::Finished) {
            return Err(PlayError::WagerLocked);
        }
        if !CHIP_DENOMINATIONS.contains(&cents) {
            return Err(PlayError::InvalidWager);
        }
        let next = self
            .pending_wager_cents
            .checked_add(cents)
            .ok_or(PlayError::MoneyOverflow)?;
        if next > self.available_cents {
            return Err(PlayError::InsufficientFunds);
        }
        if self.phase() == Phase::Finished && self.pending_wager_cents == 0 {
            self.locked_wager_cents = None;
        }
        self.pending_wager_cents = next;
        self.wager_chips.push(cents);
        Ok(())
    }

    pub fn undo_chip(&mut self) -> Result<(), PlayError> {
        if !matches!(self.phase(), Phase::Ready | Phase::Finished) {
            return Err(PlayError::WagerLocked);
        }
        let Some(cents) = self.wager_chips.pop() else {
            return Ok(());
        };
        self.pending_wager_cents -= cents;
        Ok(())
    }

    pub fn clear_wager(&mut self) -> Result<(), PlayError> {
        if !matches!(self.phase(), Phase::Ready | Phase::Finished) {
            return Err(PlayError::WagerLocked);
        }
        self.pending_wager_cents = 0;
        self.wager_chips.clear();
        Ok(())
    }

    pub fn can_afford(&self, action: Action) -> bool {
        self.additional_stake_cents(action)
            .is_ok_and(|extra| extra <= self.available_cents)
    }

    pub fn deal(&mut self) -> Result<(), PlayError> {
        if !matches!(self.phase(), Phase::Ready | Phase::Finished) {
            return Err(PlayError::WagerLocked);
        }
        if self.pending_wager_cents < MINIMUM_WAGER_CENTS
            || self.pending_wager_cents % MINIMUM_WAGER_CENTS != 0
        {
            return Err(PlayError::InvalidWager);
        }
        if self.pending_wager_cents > self.available_cents {
            return Err(PlayError::InsufficientFunds);
        }
        self.game.deal()?;
        let wager = self.pending_wager_cents;
        self.available_cents -= wager;
        self.committed_cents = wager;
        self.locked_wager_cents = Some(wager);
        self.pending_wager_cents = 0;
        self.wager_chips.clear();
        self.round_settled = false;
        self.settle_if_finished()
    }

    pub fn act(&mut self, action: Action) -> Result<(), PlayError> {
        let Some(situation) = self.game.situation() else {
            return Err(PlayError::Game(GameError::NoActiveDecision));
        };
        if !situation.allows(action) {
            return Err(PlayError::Game(GameError::IllegalAction(action)));
        }
        let extra = self.additional_stake_cents(action)?;
        let committed = self
            .committed_cents
            .checked_add(extra)
            .ok_or(PlayError::MoneyOverflow)?;
        if extra > self.available_cents {
            return Err(PlayError::ActionUnaffordable);
        }
        self.game.act(action)?;
        self.available_cents -= extra;
        self.committed_cents = committed;
        self.settle_if_finished()
    }

    pub fn reset(&mut self, bankroll_cents: i64, seed: u64) -> Result<(), PlayError> {
        validate_bankroll(bankroll_cents)?;
        let mut replacement = Self::new(seed);
        replacement.starting_bankroll_cents = bankroll_cents;
        replacement.available_cents = bankroll_cents;
        replacement.history[0].bankroll_cents = bankroll_cents;
        *self = replacement;
        Ok(())
    }
    pub fn validate(&self) -> Result<(), PlayError> {
        if self.schema_version != FREE_PLAY_SCHEMA_VERSION {
            return Err(PlayError::Version {
                found: self.schema_version,
                supported: FREE_PLAY_SCHEMA_VERSION,
            });
        }
        validate_bankroll(self.starting_bankroll_cents)?;
        if self.available_cents < 0
            || self.committed_cents < 0
            || self.available_cents > MAX_MONEY_CENTS
            || self.committed_cents > MAX_MONEY_CENTS
        {
            return Err(PlayError::InvalidSnapshot(
                "negative or unrepresentable available or committed funds".to_owned(),
            ));
        }
        let pending_sum = self
            .wager_chips
            .iter()
            .try_fold(0_i64, |sum, chip| sum.checked_add(*chip))
            .ok_or(PlayError::MoneyOverflow)?;
        if self.pending_wager_cents != pending_sum
            || self.pending_wager_cents < 0
            || self.pending_wager_cents % MINIMUM_WAGER_CENTS != 0
            || self
                .wager_chips
                .iter()
                .any(|chip| !CHIP_DENOMINATIONS.contains(chip))
            || self.pending_wager_cents > self.available_cents
        {
            return Err(PlayError::InvalidSnapshot(
                "pending wager is inconsistent".to_owned(),
            ));
        }
        if let Some(wager) = self.locked_wager_cents
            && (wager < MINIMUM_WAGER_CENTS || wager % MINIMUM_WAGER_CENTS != 0)
        {
            return Err(PlayError::InvalidSnapshot(
                "locked wager is invalid".to_owned(),
            ));
        }
        if self.history.is_empty()
            || self.history[0].round != 0
            || self.history[0].bankroll_cents != self.starting_bankroll_cents
            || self.history[0].cumulative_net_cents != 0
        {
            return Err(PlayError::InvalidSnapshot(
                "history must start at bankroll zero".to_owned(),
            ));
        }
        for pair in self.history.windows(2) {
            if pair[1].round != pair[0].round.saturating_add(1)
                || pair[1].bankroll_cents
                    != self
                        .starting_bankroll_cents
                        .checked_add(pair[1].cumulative_net_cents)
                        .ok_or(PlayError::MoneyOverflow)?
            {
                return Err(PlayError::InvalidSnapshot(
                    "history rounds are inconsistent".to_owned(),
                ));
            }
        }
        Game::from_snapshot(self.game.snapshot())
            .map_err(|error| PlayError::InvalidSnapshot(error.to_string()))?;
        let expected_committed = i64::from(self.game.stake_half_units())
            .checked_mul(self.locked_wager_cents.unwrap_or(0))
            .ok_or(PlayError::MoneyOverflow)?
            / 2;
        let historical_bankroll = self
            .history
            .last()
            .expect("history checked above")
            .bankroll_cents;
        if self.available_cents.checked_add(self.committed_cents) != Some(historical_bankroll) {
            return Err(PlayError::InvalidSnapshot(
                "funds do not match history".to_owned(),
            ));
        }
        match self.phase() {
            Phase::Ready => {
                if self.committed_cents != 0
                    || self.locked_wager_cents.is_some()
                    || !self.round_settled
                    || self.available_cents != historical_bankroll
                {
                    return Err(PlayError::InvalidSnapshot(
                        "ready funds are inconsistent".to_owned(),
                    ));
                }
            }
            Phase::Insurance | Phase::Playing => {
                if self.locked_wager_cents.is_none()
                    || self.round_settled
                    || self.pending_wager_cents != 0
                    || !self.wager_chips.is_empty()
                    || self.committed_cents != expected_committed
                    || self
                        .available_cents
                        .checked_add(self.committed_cents)
                        .is_none()
                {
                    return Err(PlayError::InvalidSnapshot(
                        "unfinished stake is inconsistent".to_owned(),
                    ));
                }
            }
            Phase::Finished => {
                if !self.round_settled
                    || self.locked_wager_cents.is_none()
                    || self.committed_cents != 0
                    || self.history.last().unwrap().bankroll_cents != self.available_cents
                {
                    return Err(PlayError::InvalidSnapshot(
                        "settled funds are inconsistent".to_owned(),
                    ));
                }
            }
        }
        Ok(())
    }

    fn additional_stake_cents(&self, action: Action) -> Result<i64, PlayError> {
        let Some(wager) = self.locked_wager_cents else {
            return Ok(0);
        };
        match action {
            Action::Double | Action::Split => Ok(wager),
            Action::Insure => Ok(wager / 2),
            _ => Ok(0),
        }
    }

    fn settle_if_finished(&mut self) -> Result<(), PlayError> {
        if self.phase() != Phase::Finished || self.round_settled {
            return Ok(());
        }
        let result =
            self.game.result.as_ref().ok_or_else(|| {
                PlayError::InvalidSnapshot("finished game has no result".to_owned())
            })?;
        let wager = self.locked_wager_cents.ok_or_else(|| {
            PlayError::InvalidSnapshot("finished game has no locked wager".to_owned())
        })?;
        let net = wager
            .checked_mul(i64::from(result.net_half_units))
            .ok_or(PlayError::MoneyOverflow)?
            / 2;
        let bankroll = self
            .available_cents
            .checked_add(self.committed_cents)
            .and_then(|value| value.checked_add(net))
            .ok_or(PlayError::MoneyOverflow)?;
        let cumulative = bankroll
            .checked_sub(self.starting_bankroll_cents)
            .ok_or(PlayError::MoneyOverflow)?;
        let round = self.history.last().map_or(0, |point| point.round);
        self.available_cents = bankroll;
        self.committed_cents = 0;
        self.history.push(PlayPoint {
            round: round.saturating_add(1),
            bankroll_cents: bankroll,
            cumulative_net_cents: cumulative,
        });
        self.round_settled = true;
        Ok(())
    }
}

pub fn parse_money_cents(input: &str) -> Result<i64, PlayError> {
    let trimmed = input.trim();
    let digits = trimmed.strip_prefix('$').unwrap_or(trimmed);
    let (whole, fraction) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty()
        || !whole.chars().all(|character| character.is_ascii_digit())
        || fraction.len() > 2
        || !fraction.chars().all(|character| character.is_ascii_digit())
    {
        return Err(PlayError::InvalidMoney);
    }
    let dollars = whole.parse::<i64>().map_err(|_| PlayError::MoneyOverflow)?;
    let cents = fraction.parse::<i64>().unwrap_or(0) * 10_i64.pow(2 - fraction.len() as u32);
    let total = dollars
        .checked_mul(100)
        .and_then(|value| value.checked_add(cents))
        .ok_or(PlayError::MoneyOverflow)?;
    validate_bankroll(total)?;
    Ok(total)
}

pub fn validate_bankroll(cents: i64) -> Result<(), PlayError> {
    if !(1..=MAX_MONEY_CENTS).contains(&cents) {
        return Err(if cents > MAX_MONEY_CENTS {
            PlayError::MoneyOverflow
        } else {
            PlayError::InvalidMoney
        });
    }
    Ok(())
}
pub fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.unsigned_abs();
    format!("{sign}${}.{:02}", cents / 100, cents % 100)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{Game, HandStatus, PlayerHand};
    use crate::model::Action;

    fn game_with_cards(player: &[u8], dealer: &[u8]) -> Game {
        let mut snapshot = Game::new(9).snapshot();
        let take = |value: u8, shoe: &mut Vec<crate::model::Card>| {
            let position = shoe
                .iter()
                .position(|card| card.rank.value() == value)
                .unwrap();
            shoe.swap_remove(position)
        };
        let player_cards = player
            .iter()
            .map(|&value| take(value, &mut snapshot.shoe))
            .collect();
        let dealer_cards = dealer
            .iter()
            .map(|&value| take(value, &mut snapshot.shoe))
            .collect();
        snapshot.dealer = dealer_cards;
        snapshot.hands = vec![PlayerHand {
            cards: player_cards,
            wager_half_units: 2,
            status: HandStatus::Playing,
            from_split: false,
            split_aces: false,
        }];
        snapshot.active = 0;
        snapshot.phase = Phase::Playing;
        snapshot.result = None;
        snapshot.insured = false;
        snapshot.splits_used = 0;
        Game::from_snapshot(snapshot).unwrap()
    }

    fn prepared_session(player: &[u8], dealer: &[u8]) -> PlaySession {
        let mut session = PlaySession::new(10);
        session.game = game_with_cards(player, dealer);
        session.available_cents = 99_500;
        session.committed_cents = 500;
        session.locked_wager_cents = Some(500);
        session.round_settled = false;
        session
    }

    #[test]
    fn money_parser_rejects_ambiguous_values_without_mutating_session() {
        let mut session = PlaySession::new(1);
        assert_eq!(parse_money_cents("1000.00"), Ok(100_000));
        for input in ["", "0", "-1", "1.001", "1e3", "$1,000"] {
            assert!(parse_money_cents(input).is_err(), "{input}");
        }
        let before = session.clone();
        assert!(session.add_chip(1).is_err());
        assert_eq!(session.pending_wager_cents, before.pending_wager_cents);
    }

    #[test]
    fn natural_surrender_and_push_have_exact_cent_results() {
        let mut natural = prepared_session(&[11, 10], &[10, 6]);
        natural.act(Action::Stand).unwrap();
        assert_eq!(natural.available_cents, 100_750);
        assert_eq!(natural.cumulative_net_cents(), 750);

        let mut surrender = prepared_session(&[10, 6], &[10, 6]);
        surrender.act(Action::Surrender).unwrap();
        assert_eq!(surrender.available_cents, 99_750);
        assert_eq!(surrender.cumulative_net_cents(), -250);

        let mut push = prepared_session(&[10, 7], &[10, 7]);
        push.act(Action::Stand).unwrap();
        assert_eq!(push.available_cents, 100_000);
        assert_eq!(push.cumulative_net_cents(), 0);
    }

    #[test]
    fn unaffordable_action_does_not_change_game_or_funds() {
        let mut session = prepared_session(&[5, 5], &[6, 10]);
        session.available_cents = 0;
        let before = serde_json::to_value(&session).unwrap();
        assert!(session.act(Action::Double).is_err());
        assert_eq!(serde_json::to_value(&session).unwrap(), before);
    }

    #[test]
    fn insurance_and_split_stakes_are_reserved_and_settled_once() {
        let mut insured = prepared_session(&[10, 6], &[11, 10]);
        insured.game.phase = Phase::Insurance;
        insured.act(Action::Insure).unwrap();
        assert_eq!(insured.committed_cents, 750);
        assert_eq!(insured.available_cents, 99_250);
        insured.act(Action::Stand).unwrap();
        assert_eq!(insured.available_cents, 100_000);
        assert_eq!(insured.committed_cents, 0);
        assert_eq!(insured.cumulative_net_cents(), 0);

        let mut split = prepared_session(&[8, 8], &[10, 6]);
        split.act(Action::Split).unwrap();
        assert_eq!(split.committed_cents, 1_000);
        assert_eq!(split.game.hands.len(), 2);
        let restored: PlaySession =
            serde_json::from_slice(&serde_json::to_vec(&split).unwrap()).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.committed_cents, 1_000);
        assert_eq!(restored.game.hands.len(), 2);
    }

    #[test]
    fn reset_replaces_history_without_fabricating_a_round() {
        let mut session = PlaySession::new(4);
        session.add_chip(500).unwrap();
        session.reset(250_000, 5).unwrap();
        assert_eq!(session.starting_bankroll_cents, 250_000);
        assert_eq!(session.available_cents, 250_000);
        assert_eq!(session.history.len(), 1);
        assert_eq!(session.history[0].cumulative_net_cents, 0);
        assert_eq!(session.phase(), Phase::Ready);
    }

    #[test]
    fn a_serialized_unfinished_round_keeps_shoe_and_phase() {
        let session = prepared_session(&[5, 5], &[6, 10]);
        let bytes = serde_json::to_vec(&session).unwrap();
        let restored: PlaySession = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.remaining_cards(), session.remaining_cards());
        assert_eq!(
            restored.game.snapshot().shoe,
            session.game.snapshot().shoe,
            "reload must preserve exact undealt shoe order"
        );
        assert_eq!(restored.game.dealer, session.game.dealer);
        assert_eq!(restored.game.hands, session.game.hands);
        assert_eq!(restored.phase(), Phase::Playing);
        assert_eq!(restored.committed_cents, 500);
        assert_eq!(restored.history.len(), 1);
    }

    #[test]
    fn serialized_finished_round_does_not_duplicate_settlement() {
        let mut session = prepared_session(&[11, 10], &[10, 6]);
        session.act(Action::Stand).unwrap();
        let bytes = serde_json::to_vec(&session).unwrap();
        let restored: PlaySession = serde_json::from_slice(&bytes).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.history.len(), 2);
        assert_eq!(restored.available_cents, 100_750);
        assert_eq!(restored.committed_cents, 0);
    }
}
