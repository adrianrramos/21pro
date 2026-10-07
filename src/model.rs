//! Small, serializable domain types shared by the engine, trainer, and desktop UI.
use serde::{Deserialize, Serialize};

pub const RULESET_ID: &str = "yaamava-six-deck-cbjn-2025-02";
pub const ASSESSMENT_ROUNDS: usize = 250;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Suit {
    Clubs,
    Diamonds,
    Hearts,
    Spades,
}
impl Suit {
    pub const ALL: [Self; 4] = [Self::Clubs, Self::Diamonds, Self::Hearts, Self::Spades];
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Clubs => "♣",
            Self::Diamonds => "♦",
            Self::Hearts => "♥",
            Self::Spades => "♠",
        }
    }
    pub const fn is_red(self) -> bool {
        matches!(self, Self::Diamonds | Self::Hearts)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rank {
    Ace,
    Two,
    Three,
    Four,
    Five,
    Six,
    Seven,
    Eight,
    Nine,
    Ten,
    Jack,
    Queen,
    King,
}
impl Rank {
    pub const ALL: [Self; 13] = [
        Self::Ace,
        Self::Two,
        Self::Three,
        Self::Four,
        Self::Five,
        Self::Six,
        Self::Seven,
        Self::Eight,
        Self::Nine,
        Self::Ten,
        Self::Jack,
        Self::Queen,
        Self::King,
    ];
    pub const fn value(self) -> u8 {
        match self {
            Self::Ace => 11,
            Self::Two => 2,
            Self::Three => 3,
            Self::Four => 4,
            Self::Five => 5,
            Self::Six => 6,
            Self::Seven => 7,
            Self::Eight => 8,
            Self::Nine => 9,
            _ => 10,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ace => "A",
            Self::Two => "2",
            Self::Three => "3",
            Self::Four => "4",
            Self::Five => "5",
            Self::Six => "6",
            Self::Seven => "7",
            Self::Eight => "8",
            Self::Nine => "9",
            Self::Ten => "10",
            Self::Jack => "J",
            Self::Queen => "Q",
            Self::King => "K",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub rank: Rank,
    pub suit: Suit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandValue {
    pub total: u8,
    pub soft: bool,
}
pub fn hand_value(cards: &[Card]) -> HandValue {
    let mut total: u16 = cards.iter().map(|card| u16::from(card.rank.value())).sum();
    let mut aces = cards.iter().filter(|card| card.rank == Rank::Ace).count();
    while total > 21 && aces > 0 {
        total -= 10;
        aces -= 1;
    }
    HandValue {
        total: total.min(u16::from(u8::MAX)) as u8,
        soft: aces > 0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    Hit,
    Stand,
    Double,
    Split,
    Surrender,
    Insure,
    DeclineInsurance,
}
impl Action {
    pub const ALL: [Self; 7] = [
        Self::Hit,
        Self::Stand,
        Self::Double,
        Self::Split,
        Self::Surrender,
        Self::Insure,
        Self::DeclineInsurance,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hit => "Hit",
            Self::Stand => "Stand",
            Self::Double => "Double",
            Self::Split => "Split",
            Self::Surrender => "Surrender",
            Self::Insure => "Take insurance",
            Self::DeclineInsurance => "No insurance",
        }
    }
    pub const fn short(self) -> &'static str {
        match self {
            Self::Hit => "H",
            Self::Stand => "S",
            Self::Double => "D",
            Self::Split => "P",
            Self::Surrender => "R",
            Self::Insure => "I",
            Self::DeclineInsurance => "N",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum HandKind {
    Hard,
    Soft,
    Pair,
    Insurance,
}
impl HandKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Hard => "Hard totals",
            Self::Soft => "Soft totals",
            Self::Pair => "Pairs",
            Self::Insurance => "Insurance",
        }
    }
}

/// Legal actions are part of a learning item: a three-card 11 cannot be doubled,
/// and a post-split 16 cannot be surrendered like an original two-card 16.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Situation {
    pub kind: HandKind,
    /// Hard/soft total, or the pair's card value (aces = 11).
    pub value: u8,
    /// Dealer upcard value (aces = 11).
    pub dealer: u8,
    pub can_double: bool,
    pub can_split: bool,
    pub can_surrender: bool,
    pub split_aces: bool,
}
impl Situation {
    pub fn allows(self, action: Action) -> bool {
        if self.kind == HandKind::Insurance {
            return matches!(action, Action::Insure | Action::DeclineInsurance);
        }
        match action {
            Action::Hit => !self.split_aces,
            Action::Stand => true,
            Action::Double => self.can_double,
            Action::Split => self.can_split,
            Action::Surrender => self.can_surrender,
            Action::Insure | Action::DeclineInsurance => false,
        }
    }
    pub fn label(self) -> String {
        let dealer = if self.dealer == 11 {
            "A".to_owned()
        } else {
            self.dealer.to_string()
        };
        let hand = match self.kind {
            HandKind::Hard => format!("Hard {}", self.value),
            HandKind::Soft => format!("Soft {}", self.value),
            HandKind::Pair if self.value == 11 => "A,A".to_owned(),
            HandKind::Pair => format!("{},{}", self.value, self.value),
            HandKind::Insurance => "Insurance".to_owned(),
        };
        format!("{hand} vs {dealer}")
    }
    pub fn context(self) -> &'static str {
        if self.kind == HandKind::Insurance {
            "Insurance decision"
        } else if self.split_aces {
            "Split aces · one card only; resplitting allowed within the limit"
        } else if self.kind == HandKind::Pair && !self.can_split {
            "Split limit reached"
        } else if self.can_surrender {
            "Original two-card hand · surrender available"
        } else if self.can_double {
            "Post-split hand · no surrender"
        } else {
            "Hit/stand only · no double or surrender"
        }
    }
}
