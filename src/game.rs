//! Six-deck H17, DAS, late surrender, resplit aces, four hands, 3:2 blackjack.
//!
//! No-look / original-bet-only: the dealer's hole card never changes legal
//! actions. Reveal a natural only at settlement, refund ALL additional split
//! and double stakes (even on busted hands), and lose at most the original
//! two half-units on the main bet. Late surrender is conditional: a dealer
//! natural takes the full original bet. Insurance is a separate one-half-unit
//! side bet paying two to one, not part of that main-bet loss cap.
use crate::model::{Action, Card, HandKind, Rank, Situation, Suit, hand_value};
use rand::{SeedableRng, rngs::SmallRng, seq::SliceRandom};

const BASE_BET: i32 = 2;
const CUT_CARD: usize = 68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Ready,
    Insurance,
    Playing,
    Finished,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandStatus {
    Playing,
    Stood,
    Busted,
    Surrendered,
    Settled,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerHand {
    pub cards: Vec<Card>,
    pub wager_half_units: i32,
    pub status: HandStatus,
    pub from_split: bool,
    pub split_aces: bool,
}
impl PlayerHand {
    fn new(cards: Vec<Card>, from_split: bool, split_aces: bool) -> Self {
        Self {
            cards,
            wager_half_units: BASE_BET,
            status: HandStatus::Playing,
            from_split,
            split_aces,
        }
    }
    fn natural(&self) -> bool {
        !self.from_split && self.cards.len() == 2 && hand_value(&self.cards).total == 21
    }
    fn pair(&self) -> bool {
        self.cards.len() == 2 && self.cards[0].rank.value() == self.cards[1].rank.value()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundResult {
    pub net_half_units: i32,
    pub outcomes: Vec<HandOutcome>,
    pub dealer_blackjack: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HandOutcome {
    pub label: String,
    pub net_half_units: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GameError {
    #[error("Finish the current round before dealing again.")]
    RoundInProgress,
    #[error("There is no active decision in this round.")]
    NoActiveDecision,
    #[error("{0:?} is not legal for this hand.")]
    IllegalAction(Action),
    #[error("Unreachable practice situation: {0}")]
    InvalidPractice(&'static str),
}

#[derive(Debug, Clone)]
pub struct Game {
    /// Upcard first; the renderer must conceal index 1 until Phase::Finished.
    pub dealer: Vec<Card>,
    pub hands: Vec<PlayerHand>,
    pub active: usize,
    pub phase: Phase,
    pub result: Option<RoundResult>,
    pub is_practice: bool,
    shoe: Vec<Card>,
    rng: SmallRng,
    insured: bool,
    // Practice may reserve prior split capacity without inventing extra hands.
    splits_used: usize,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        let mut game = Self {
            dealer: Vec::with_capacity(12),
            hands: Vec::with_capacity(4),
            active: 0,
            phase: Phase::Ready,
            result: None,
            is_practice: false,
            shoe: Vec::with_capacity(312),
            rng: SmallRng::seed_from_u64(seed),
            insured: false,
            splits_used: 0,
        };
        game.shuffle();
        game
    }

    fn shuffle(&mut self) {
        self.shoe.clear();
        for _ in 0..6 {
            for suit in Suit::ALL {
                for rank in Rank::ALL {
                    self.shoe.push(Card { rank, suit });
                }
            }
        }
        self.shoe.shuffle(&mut self.rng);
    }

    fn draw(&mut self) -> Card {
        // A round starts above the cut card; with one seat and at most four
        // hands, the six-deck rank inventory cannot exhaust that reserve.
        self.shoe
            .pop()
            .expect("six-deck cut-card reserve exhausted")
    }

    pub fn remaining_cards(&self) -> usize {
        self.shoe.len()
    }

    pub fn deal(&mut self) -> Result<(), GameError> {
        if matches!(self.phase, Phase::Insurance | Phase::Playing) {
            return Err(GameError::RoundInProgress);
        }
        if self.shoe.len() <= CUT_CARD {
            self.shuffle();
        }
        self.dealer.clear();
        self.hands.clear();
        self.result = None;
        self.active = 0;
        self.splits_used = 0;
        self.insured = false;
        self.is_practice = false;
        let first = self.draw();
        let upcard = self.draw();
        let second = self.draw();
        let hole = self.draw();
        self.hands
            .push(PlayerHand::new(vec![first, second], false, false));
        self.dealer.extend([upcard, hole]);
        self.phase = if upcard.rank == Rank::Ace {
            Phase::Insurance
        } else {
            Phase::Playing
        };
        if self.phase == Phase::Playing {
            self.advance();
        }
        Ok(())
    }

    pub fn situation(&self) -> Option<Situation> {
        if self.phase == Phase::Insurance {
            return Some(Situation {
                kind: HandKind::Insurance,
                value: 0,
                dealer: 11,
                can_double: false,
                can_split: false,
                can_surrender: false,
                split_aces: false,
            });
        }
        if self.phase != Phase::Playing {
            return None;
        }
        let hand = self.hands.get(self.active)?;
        if hand.status != HandStatus::Playing {
            return None;
        }
        let value = hand_value(&hand.cards);
        let pair = hand.pair();
        Some(Situation {
            kind: if pair {
                HandKind::Pair
            } else if value.soft {
                HandKind::Soft
            } else {
                HandKind::Hard
            },
            value: if pair {
                hand.cards[0].rank.value()
            } else {
                value.total
            },
            dealer: self.dealer[0].rank.value(),
            can_double: hand.cards.len() == 2 && !hand.split_aces,
            can_split: pair && self.splits_used < 3,
            can_surrender: hand.cards.len() == 2 && !hand.from_split,
            split_aces: hand.split_aces,
        })
    }

    pub fn act(&mut self, action: Action) -> Result<(), GameError> {
        let situation = self.situation().ok_or(GameError::NoActiveDecision)?;
        // Check before any draw, RNG advance, bet change, or other mutation.
        if !situation.allows(action) {
            return Err(GameError::IllegalAction(action));
        }
        match action {
            Action::Insure | Action::DeclineInsurance => {
                self.insured = action == Action::Insure;
                self.phase = Phase::Playing;
            }
            Action::Hit => {
                let card = self.draw();
                self.hands[self.active].cards.push(card);
            }
            Action::Stand => self.hands[self.active].status = HandStatus::Stood,
            Action::Double => {
                let card = self.draw();
                let hand = &mut self.hands[self.active];
                hand.wager_half_units *= 2;
                hand.cards.push(card);
                hand.status = if hand_value(&hand.cards).total > 21 {
                    HandStatus::Busted
                } else {
                    HandStatus::Stood
                };
            }
            Action::Surrender => self.hands[self.active].status = HandStatus::Surrendered,
            Action::Split => {
                let first_draw = self.draw();
                let second_draw = self.draw();
                let hand = &mut self.hands[self.active];
                let second = hand.cards.pop().expect("split requires a pair");
                let aces = second.rank == Rank::Ace;
                hand.cards.push(first_draw);
                hand.from_split = true;
                hand.split_aces = aces;
                self.hands.insert(
                    self.active + 1,
                    PlayerHand::new(vec![second, second_draw], true, aces),
                );
                self.splits_used += 1;
            }
        }
        self.advance();
        Ok(())
    }

    fn advance(&mut self) {
        while self.active < self.hands.len() {
            let hand = &mut self.hands[self.active];
            if hand.status == HandStatus::Playing {
                let total = hand_value(&hand.cards).total;
                if total > 21 {
                    hand.status = HandStatus::Busted;
                } else if total == 21 || (hand.split_aces && !hand.pair()) {
                    hand.status = HandStatus::Stood;
                }
            }
            if hand.status == HandStatus::Playing {
                return;
            }
            self.active += 1;
        }
        self.settle();
    }

    fn settle(&mut self) {
        let dealer_blackjack = self.dealer.len() == 2 && hand_value(&self.dealer).total == 21;
        // No need to draw against only busts, surrenders, or a natural.
        if !dealer_blackjack
            && self
                .hands
                .iter()
                .any(|hand| hand.status == HandStatus::Stood && !hand.natural())
        {
            loop {
                let value = hand_value(&self.dealer);
                if value.total > 17 || (value.total == 17 && !value.soft) {
                    break;
                }
                let card = self.draw();
                self.dealer.push(card);
            }
        }
        let dealer_total = hand_value(&self.dealer).total;
        let mut outcomes = Vec::with_capacity(self.hands.len() + usize::from(self.insured));
        for (index, hand) in self.hands.iter_mut().enumerate() {
            let total = hand_value(&hand.cards).total;
            let (net, label) = if dealer_blackjack {
                if hand.natural() {
                    (0, "Blackjack push")
                } else if index == 0 {
                    (
                        -BASE_BET,
                        "Dealer blackjack · original bet lost, extra stakes returned",
                    )
                } else {
                    (0, "Dealer blackjack · split stake returned")
                }
            } else if hand.natural() {
                (3, "Blackjack · 3:2")
            } else if hand.status == HandStatus::Surrendered {
                (-hand.wager_half_units / 2, "Late surrender")
            } else if hand.status == HandStatus::Busted {
                (-hand.wager_half_units, "Bust")
            } else if dealer_total > 21 || total > dealer_total {
                (hand.wager_half_units, "Win")
            } else if total == dealer_total {
                (0, "Push")
            } else {
                (-hand.wager_half_units, "Loss")
            };
            outcomes.push(HandOutcome {
                label: format!("Hand {}: {label}", index + 1),
                net_half_units: net,
            });
            hand.status = HandStatus::Settled;
        }
        if self.insured {
            outcomes.push(HandOutcome {
                label: "Insurance".to_owned(),
                net_half_units: if dealer_blackjack { 2 } else { -1 },
            });
        }
        self.result = Some(RoundResult {
            net_half_units: outcomes.iter().map(|o| o.net_half_units).sum(),
            outcomes,
            dealer_blackjack,
        });
        self.phase = Phase::Finished;
    }

    /// Construct a real target from a freshly shuffled six-deck shoe. Forced
    /// values consume randomly ordered physical cards; the hole card is drawn
    /// without conditioning on its rank. A split-limit drill reserves capacity
    /// (as explained by Situation::context), not fictitious live sibling hands.
    pub fn practice(target: Situation, seed: u64) -> Result<Self, GameError> {
        let invalid = GameError::InvalidPractice;
        if !(2..=11).contains(&target.dealer) {
            return Err(invalid("dealer upcard must be 2 through ace"));
        }
        let mut game = Self::new(seed);
        game.is_practice = true;
        if target.kind == HandKind::Insurance {
            let canonical = Situation {
                kind: HandKind::Insurance,
                value: 0,
                dealer: 11,
                can_double: false,
                can_split: false,
                can_surrender: false,
                split_aces: false,
            };
            if target != canonical {
                return Err(invalid(
                    "insurance uses value 0, dealer ace, and no hand-action flags",
                ));
            }
            let up = game.take_value(11)?;
            let first = game.draw();
            let second = game.draw();
            let hole = game.draw();
            game.dealer.extend([up, hole]);
            game.hands
                .push(PlayerHand::new(vec![first, second], false, false));
            game.phase = Phase::Insurance;
            return Ok(game);
        }
        if target.can_surrender && (!target.can_double || target.split_aces) {
            return Err(invalid(
                "surrender requires an original two-card hand with doubling available",
            ));
        }
        if target.can_split && target.kind != HandKind::Pair {
            return Err(invalid("only a two-card pair may split"));
        }
        if target.split_aces
            && (target.kind != HandKind::Pair
                || target.value != 11
                || target.can_double
                || target.can_surrender)
        {
            return Err(invalid(
                "only a pair of split aces has an actionable one-card-only decision",
            ));
        }
        if target.kind == HandKind::Pair && !target.can_double && !target.split_aces {
            return Err(invalid("an ordinary two-card pair always permits doubling"));
        }
        if target.kind == HandKind::Pair && !target.can_split && target.can_surrender {
            return Err(invalid(
                "an original pair cannot already have exhausted split capacity",
            ));
        }
        let (values, length) = practice_values(target).ok_or(invalid(
            "no playable card composition has this total and action availability",
        ))?;
        let mut cards = Vec::with_capacity(length);
        for &value in &values[..length] {
            cards.push(game.take_value(value)?);
        }
        let up = game.take_value(target.dealer)?;
        let hole = game.draw();
        game.dealer.extend([up, hole]);
        let from_split = target.split_aces || (target.can_double && !target.can_surrender);
        game.splits_used = if target.kind == HandKind::Pair && !target.can_split {
            3
        } else {
            usize::from(from_split)
        };
        game.hands
            .push(PlayerHand::new(cards, from_split, target.split_aces));
        game.phase = Phase::Playing;
        if game.situation() != Some(target) {
            return Err(invalid(
                "the requested action flags cannot coexist in this ruleset",
            ));
        }
        Ok(game)
    }

    fn take_value(&mut self, value: u8) -> Result<Card, GameError> {
        let position = self
            .shoe
            .iter()
            .position(|card| card.rank.value() == value)
            .ok_or(GameError::InvalidPractice(
                "the requested cards exceed the six-deck inventory",
            ))?;
        Ok(self.shoe.swap_remove(position))
    }
}

fn practice_values(target: Situation) -> Option<([u8; 3], usize)> {
    if target.kind == HandKind::Pair {
        return (2..=11)
            .contains(&target.value)
            .then_some(([target.value; 3], 2));
    }
    if !(4..=20).contains(&target.value) {
        return None;
    }
    // Every actionable non-pair total reachable after hitting has a three-card
    // representative. Reject soft 12 / hard 4–5 rather than silently substituting.
    let length = if target.can_double { 2 } else { 3 };
    for first in 2..=11 {
        for second in 2..=11 {
            for third in 2..=11 {
                if length == 2 && (first == second || third != 2) {
                    continue;
                }
                let values = [first, second, third];
                let values = &values[..length];
                let mut total: u8 = values.iter().sum();
                let mut aces = values.iter().filter(|&&v| v == 11).count();
                while total > 21 && aces > 0 {
                    total -= 10;
                    aces -= 1;
                }
                let soft = aces > 0;
                if total == target.value && soft == (target.kind == HandKind::Soft) {
                    return Some(([first, second, third], length));
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::recommendation;

    fn rig(values: &[u8]) -> Game {
        let mut game = Game::new(7);
        let cards: Vec<_> = values
            .iter()
            .map(|&v| game.take_value(v).unwrap())
            .collect();
        game.shoe.extend(cards.into_iter().rev());
        game.deal().unwrap();
        game
    }
    fn finish(game: &mut Game) {
        for _ in 0..100 {
            let Some(s) = game.situation() else {
                break;
            };
            game.act(recommendation(s).action).unwrap();
        }
        assert_eq!(game.phase, Phase::Finished);
        let result = game.result.as_ref().unwrap();
        assert_eq!(
            result.net_half_units,
            result
                .outcomes
                .iter()
                .map(|o| o.net_half_units)
                .sum::<i32>()
        );
    }
    fn net(game: &Game) -> i32 {
        game.result.as_ref().unwrap().net_half_units
    }
    fn target(kind: HandKind, value: u8) -> Situation {
        Situation {
            kind,
            value,
            dealer: 6,
            can_double: true,
            can_split: kind == HandKind::Pair,
            can_surrender: true,
            split_aces: false,
        }
    }
    fn inventory(game: &Game) {
        for rank in Rank::ALL {
            for suit in Suit::ALL {
                let count = game
                    .shoe
                    .iter()
                    .chain(&game.dealer)
                    .chain(game.hands.iter().flat_map(|h| &h.cards))
                    .filter(|c| c.rank == rank && c.suit == suit)
                    .count();
                assert_eq!(count, 6, "{rank:?} {suit:?}");
            }
        }
    }

    #[test]
    fn natural_three_to_two_and_natural_push() {
        let game = rig(&[11, 10, 10, 9]);
        assert_eq!(game.phase, Phase::Finished);
        assert_eq!(net(&game), 3);
        assert_eq!(game.remaining_cards(), 308);
        let mut push = rig(&[11, 11, 10, 10]);
        assert_eq!(push.phase, Phase::Insurance);
        push.act(Action::DeclineInsurance).unwrap();
        assert_eq!(net(&push), 0);
        assert!(push.result.unwrap().dealer_blackjack);
    }

    #[test]
    fn no_look_double_refund_and_insurance_are_separate() {
        let mut game = rig(&[5, 11, 6, 10, 10]);
        game.act(Action::Insure).unwrap();
        assert_eq!(game.phase, Phase::Playing);
        assert!(game.situation().unwrap().can_double);
        game.act(Action::Double).unwrap();
        assert_eq!(game.hands[0].cards.len(), 3);
        assert_eq!(game.hands[0].wager_half_units, 4);
        let result = game.result.unwrap();
        assert_eq!(result.net_half_units, 0);
        assert_eq!(result.outcomes[0].net_half_units, -2);
        assert_eq!(result.outcomes[1].net_half_units, 2);
    }

    #[test]
    fn insurance_loss_and_blackjack_even_money_arithmetic() {
        let mut natural = rig(&[11, 11, 10, 9]);
        natural.act(Action::Insure).unwrap();
        assert_eq!(net(&natural), 2);
        let mut push = rig(&[11, 11, 10, 10]);
        push.act(Action::Insure).unwrap();
        assert_eq!(net(&push), 2);
        let mut loss = rig(&[10, 11, 7, 9]);
        loss.act(Action::Insure).unwrap();
        loss.act(Action::Stand).unwrap();
        assert_eq!(net(&loss), -3);
    }

    #[test]
    fn late_surrender_is_conditional_and_never_after_hit_or_split() {
        for (hole, expected) in [(10, -2), (9, -1)] {
            let mut game = rig(&[10, 11, 6, hole]);
            game.act(Action::DeclineInsurance).unwrap();
            game.act(Action::Surrender).unwrap();
            assert_eq!(net(&game), expected);
        }
        let mut hit = rig(&[2, 10, 3, 8, 2]);
        hit.act(Action::Hit).unwrap();
        assert!(!hit.situation().unwrap().can_surrender);
        assert_eq!(
            hit.act(Action::Surrender),
            Err(GameError::IllegalAction(Action::Surrender))
        );
        let mut split = rig(&[8, 10, 8, 8, 2, 3]);
        split.act(Action::Split).unwrap();
        assert!(!split.situation().unwrap().can_surrender);
        assert!(split.situation().unwrap().can_double);
    }

    #[test]
    fn original_bet_only_refunds_busted_split_and_double_stakes() {
        let mut game = rig(&[8, 10, 8, 11, 10, 3, 10, 10]);
        game.act(Action::Split).unwrap();
        game.act(Action::Hit).unwrap(); // First split hand busts at 28.
        game.act(Action::Double).unwrap();
        assert_eq!(net(&game), -2);
        assert_eq!(game.result.as_ref().unwrap().outcomes[1].net_half_units, 0);
        assert_eq!(
            game.hands.iter().map(|h| h.wager_half_units).sum::<i32>(),
            6
        );
    }

    #[test]
    fn resplit_aces_four_hand_cap_and_no_hit_or_double() {
        let mut game = rig(&[11, 6, 11, 10, 11, 11, 11, 11, 11, 11, 10]);
        for _ in 0..3 {
            game.act(Action::Split).unwrap();
            let s = game.situation().unwrap();
            assert!(s.split_aces);
            assert!(!s.can_double && !s.can_surrender);
            assert_eq!(
                game.act(Action::Hit),
                Err(GameError::IllegalAction(Action::Hit))
            );
            assert_eq!(
                game.act(Action::Double),
                Err(GameError::IllegalAction(Action::Double))
            );
        }
        assert_eq!(game.hands.len(), 4);
        assert!(!game.situation().unwrap().can_split);
        assert_eq!(
            game.act(Action::Split),
            Err(GameError::IllegalAction(Action::Split))
        );
        for _ in 0..4 {
            game.act(Action::Stand).unwrap();
        }
        assert_eq!(game.phase, Phase::Finished);
        assert!(game.hands.iter().all(|h| h.cards.len() == 2));
        inventory(&game);
    }

    #[test]
    fn nonace_split_cap_and_das() {
        let mut game = rig(&[2, 6, 2, 10, 2, 2, 2, 2, 2, 2, 10, 10, 10, 10, 10]);
        for _ in 0..3 {
            game.act(Action::Split).unwrap();
        }
        assert_eq!(game.hands.len(), 4);
        assert!(!game.situation().unwrap().can_split);
        for _ in 0..4 {
            game.act(Action::Double).unwrap();
        }
        assert_eq!(net(&game), 16);
        assert!(
            game.hands
                .iter()
                .all(|h| h.wager_half_units == 4 && h.cards.len() == 3)
        );
    }

    #[test]
    fn split_twenty_one_is_not_blackjack() {
        let mut game = rig(&[11, 10, 11, 10, 10, 10]);
        game.act(Action::Split).unwrap();
        assert_eq!(game.phase, Phase::Finished);
        assert_eq!(net(&game), 4);
        let mut push = rig(&[10, 10, 10, 11, 11, 11]);
        push.act(Action::Split).unwrap();
        assert_eq!(net(&push), -2);
    }

    #[test]
    fn dealer_hits_soft_seventeen_and_stands_hard_seventeen() {
        let mut soft = rig(&[10, 6, 8, 11, 2]);
        soft.act(Action::Stand).unwrap();
        assert_eq!(soft.dealer.len(), 3);
        assert_eq!(net(&soft), -2);
        let mut hard = rig(&[10, 7, 8, 10, 2]);
        hard.act(Action::Stand).unwrap();
        assert_eq!(hard.dealer.len(), 2);
        assert_eq!(net(&hard), 2);
    }

    #[test]
    fn multiple_aces_revalue_after_hits() {
        let mut game = rig(&[11, 6, 11, 10, 9, 10]);
        assert_eq!(hand_value(&game.hands[0].cards).total, 12);
        game.act(Action::Hit).unwrap();
        assert_eq!(hand_value(&game.hands[0].cards).total, 21);
        assert_eq!(net(&game), 2);
        let cards = [11, 11, 11, 9].map(|v| Card {
            rank: if v == 11 { Rank::Ace } else { Rank::Nine },
            suit: Suit::Clubs,
        });
        assert_eq!(hand_value(&cards).total, 12);
        assert!(!hand_value(&cards).soft);
    }

    #[test]
    fn illegal_action_and_deal_leave_entire_state_unchanged() {
        let mut game = rig(&[10, 6, 7, 10]);
        let before = format!("{game:?}");
        assert_eq!(
            game.act(Action::Split),
            Err(GameError::IllegalAction(Action::Split))
        );
        assert_eq!(format!("{game:?}"), before);
        assert_eq!(game.deal(), Err(GameError::RoundInProgress));
        assert_eq!(format!("{game:?}"), before);
        game.act(Action::Hit).unwrap(); // Deliberately non-strategy move still applies.
        assert_eq!(game.hands[0].cards.len(), 3);
    }

    #[test]
    fn hole_card_never_changes_legal_actions() {
        let mut natural = rig(&[8, 11, 8, 10]);
        let mut other = rig(&[8, 11, 8, 9]);
        assert_eq!(natural.situation(), other.situation());
        natural.act(Action::DeclineInsurance).unwrap();
        other.act(Action::DeclineInsurance).unwrap();
        assert_eq!(natural.situation(), other.situation());
        assert_eq!(natural.phase, Phase::Playing);
    }

    #[test]
    fn cut_card_reshuffles_only_before_next_round() {
        let mut game = rig(&[10, 6, 7, 10, 10]);
        game.shoe.truncate(68);
        let before = game.remaining_cards();
        finish(&mut game);
        assert!(game.remaining_cards() <= before);
        game.deal().unwrap();
        assert!(game.remaining_cards() >= 308);
        let mut above = Game::new(5);
        above.shoe.truncate(69);
        above.deal().unwrap();
        assert_eq!(above.remaining_cards(), 65);
    }

    #[test]
    fn practice_round_trips_every_supported_legal_context() {
        let mut cases = Vec::new();
        for dealer in 2..=11 {
            for value in 5..=19 {
                for can_surrender in [false, true] {
                    cases.push(Situation {
                        dealer,
                        can_surrender,
                        ..target(HandKind::Hard, value)
                    });
                }
            }
            for value in 6..=20 {
                cases.push(Situation {
                    dealer,
                    can_double: false,
                    can_surrender: false,
                    ..target(HandKind::Hard, value)
                });
            }
            for value in 13..=20 {
                for (can_double, can_surrender) in [(true, true), (true, false), (false, false)] {
                    cases.push(Situation {
                        dealer,
                        can_double,
                        can_surrender,
                        ..target(HandKind::Soft, value)
                    });
                }
            }
            for value in 2..=11 {
                for (can_split, can_surrender) in [(true, true), (true, false), (false, false)] {
                    cases.push(Situation {
                        dealer,
                        can_split,
                        can_surrender,
                        ..target(HandKind::Pair, value)
                    });
                }
            }
            for can_split in [false, true] {
                cases.push(Situation {
                    dealer,
                    can_split,
                    can_double: false,
                    can_surrender: false,
                    split_aces: true,
                    ..target(HandKind::Pair, 11)
                });
            }
        }
        cases.push(Situation {
            kind: HandKind::Insurance,
            value: 0,
            dealer: 11,
            can_double: false,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        });
        for (seed, s) in cases.into_iter().enumerate() {
            let mut game = Game::practice(s, seed as u64).unwrap_or_else(|e| panic!("{s:?}: {e}"));
            assert_eq!(game.situation(), Some(s));
            assert!(game.is_practice);
            assert_eq!(game.hands.len(), 1);
            inventory(&game);
            finish(&mut game);
            inventory(&game);
        }
    }

    #[test]
    fn practice_rejects_unreachable_and_does_not_condition_hole_card() {
        for s in [
            target(HandKind::Hard, 4),
            target(HandKind::Hard, 20),
            target(HandKind::Soft, 12),
            target(HandKind::Soft, 21),
            Situation {
                can_split: false,
                ..target(HandKind::Pair, 8)
            },
            Situation {
                can_double: false,
                ..target(HandKind::Hard, 11)
            },
        ] {
            assert!(Game::practice(s, 1).is_err(), "{s:?}");
        }
        let s = Situation {
            dealer: 11,
            ..target(HandKind::Hard, 16)
        };
        let mut naturals = 0;
        let mut suits = Vec::new();
        let mut ten_ranks = Vec::new();
        for seed in 0..100 {
            let game = Game::practice(s, seed).unwrap();
            naturals += usize::from(game.dealer[1].rank.value() == 10);
            suits.push(game.hands[0].cards[0].suit);
            if game.dealer[1].rank.value() == 10 {
                ten_ranks.push(game.dealer[1].rank);
            }
        }
        assert!(naturals > 0 && naturals < 100);
        assert!(suits.iter().any(|suit| *suit != suits[0]));
        assert!(ten_ranks.iter().any(|rank| *rank != ten_ranks[0]));
    }
}
