//! Offline training-suite state machines and grading.
//!
//! The renderer consumes these types but does not implement count arithmetic,
//! strategy grading, timing, reports, or persistence rules.

use std::collections::BTreeMap;

use rand::{Rng, SeedableRng, rngs::SmallRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};

use crate::{
    counting::{quarter_deck_alternate, running_count, true_count_divisor, truncated_true_count},
    game::{Game, GameError, Phase},
    model::{Action, Card, HandKind, Rank, Situation, Suit},
    strategy::{self, IndexPlay, StrategySource},
};

pub const SUITE_RULESET_ID: &str = crate::model::RULESET_ID;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolId {
    RunningCount,
    TrueCount,
    BasicStrategy,
    CoreIndexPlays,
    TablePractice,
    Assessment,
    PressureTable,
}

impl ToolId {
    pub const ALL: [Self; 7] = [
        Self::RunningCount,
        Self::TrueCount,
        Self::BasicStrategy,
        Self::CoreIndexPlays,
        Self::TablePractice,
        Self::Assessment,
        Self::PressureTable,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::RunningCount => "Running Count",
            Self::TrueCount => "True Count",
            Self::BasicStrategy => "Basic Strategy",
            Self::CoreIndexPlays => "Core Index Plays",
            Self::TablePractice => "Table Practice",
            Self::Assessment => "Assessment",
            Self::PressureTable => "Pressure Table",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub enum GradeBucket {
    BasicStrategy,
    Deviation,
    Bet,
    Count,
    Insurance,
    Math,
    Timing,
    Mispay,
}

impl GradeBucket {
    pub const ALL: [Self; 8] = [
        Self::BasicStrategy,
        Self::Deviation,
        Self::Bet,
        Self::Count,
        Self::Insurance,
        Self::Math,
        Self::Timing,
        Self::Mispay,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::BasicStrategy => "Basic strategy",
            Self::Deviation => "Core indexes",
            Self::Bet => "Betting",
            Self::Count => "Counting",
            Self::Insurance => "Insurance",
            Self::Math => "Math",
            Self::Timing => "Timing",
            Self::Mispay => "Mispay",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BucketStats {
    pub attempts: u32,
    pub correct: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SuiteStats {
    pub attempts: u32,
    pub correct: u32,
    pub elapsed_secs: u64,
    pub current_streak: u32,
    pub longest_streak: u32,
    #[serde(default)]
    pub buckets: BTreeMap<GradeBucket, BucketStats>,
    #[serde(default)]
    pub current_attempts: u32,
    #[serde(default)]
    pub current_correct: u32,
    #[serde(default)]
    pub current_elapsed_secs: u64,
    #[serde(default)]
    pub current_buckets: BTreeMap<GradeBucket, BucketStats>,
}

impl SuiteStats {
    pub fn accuracy(&self) -> f32 {
        accuracy(self.correct, self.attempts)
    }

    pub fn current_accuracy(&self) -> f32 {
        accuracy(self.current_correct, self.current_attempts)
    }

    pub fn record(&mut self, bucket: GradeBucket, correct: bool) {
        record_bucket(
            &mut self.attempts,
            &mut self.correct,
            &mut self.longest_streak,
            &mut self.current_streak,
            &mut self.buckets,
            bucket,
            correct,
        );
        self.current_attempts = self.current_attempts.saturating_add(1);
        self.current_correct = self.current_correct.saturating_add(u32::from(correct));
        let bucket_stats = self.current_buckets.entry(bucket).or_default();
        bucket_stats.attempts = bucket_stats.attempts.saturating_add(1);
        bucket_stats.correct = bucket_stats.correct.saturating_add(u32::from(correct));
    }

    pub fn reset_current(&mut self) {
        self.current_attempts = 0;
        self.current_correct = 0;
        self.current_elapsed_secs = 0;
        self.current_streak = 0;
        self.current_buckets.clear();
    }
}

fn accuracy(correct: u32, attempts: u32) -> f32 {
    if attempts == 0 {
        1.0
    } else {
        correct as f32 / attempts as f32
    }
}

fn record_bucket(
    attempts: &mut u32,
    correct_count: &mut u32,
    longest_streak: &mut u32,
    current_streak: &mut u32,
    buckets: &mut BTreeMap<GradeBucket, BucketStats>,
    bucket: GradeBucket,
    correct: bool,
) {
    *attempts = attempts.saturating_add(1);
    *correct_count = correct_count.saturating_add(u32::from(correct));
    let bucket_stats = buckets.entry(bucket).or_default();
    bucket_stats.attempts = bucket_stats.attempts.saturating_add(1);
    bucket_stats.correct = bucket_stats.correct.saturating_add(u32::from(correct));
    if correct {
        *current_streak = current_streak.saturating_add(1);
        *longest_streak = (*longest_streak).max(*current_streak);
    } else {
        *current_streak = 0;
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolSettings {
    /// Deck count used by the True Count drill.
    pub true_count_decks: usize,
    /// Selects the documented half-deck divisor for True Count.
    pub half_deck: bool,
    /// Running-count checkpoint interval; `None` means the end-of-shoe prompt.
    pub count_every: Option<usize>,
    /// True Count session timeout in seconds.
    pub true_count_duration_secs: u64,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            true_count_decks: 2,
            half_deck: false,
            count_every: Some(5),
            true_count_duration_secs: 90,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSession {
    pub tool: ToolId,
    pub elapsed_secs: u64,
    pub finished: bool,
    pub passed: Option<bool>,
    pub attempts: u32,
    pub correct: u32,
    pub mistakes: BTreeMap<GradeBucket, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrillFeedback {
    pub chosen: Action,
    pub expected: Action,
    pub correct: bool,
    pub source: StrategySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyScenario {
    pub situation: Situation,
    pub card_count: u8,
}

/// Generates legal opening and multi-card situations from the local strategy function.
pub fn basic_strategy_catalog(include_multi_card: bool) -> Vec<StrategyScenario> {
    let mut scenarios = Vec::new();
    for dealer in 2..=11 {
        for value in 4..=21 {
            scenarios.push(StrategyScenario {
                situation: Situation {
                    kind: HandKind::Hard,
                    value,
                    dealer,
                    can_double: true,
                    can_split: false,
                    can_surrender: true,
                    split_aces: false,
                },
                card_count: 2,
            });
        }
        for value in 13..=21 {
            scenarios.push(StrategyScenario {
                situation: Situation {
                    kind: HandKind::Soft,
                    value,
                    dealer,
                    can_double: true,
                    can_split: false,
                    can_surrender: false,
                    split_aces: false,
                },
                card_count: 2,
            });
        }
        for value in 2..=11 {
            scenarios.push(StrategyScenario {
                situation: Situation {
                    kind: HandKind::Pair,
                    value,
                    dealer,
                    can_double: true,
                    can_split: true,
                    can_surrender: true,
                    split_aces: false,
                },
                card_count: 2,
            });
        }
        if include_multi_card {
            for value in 5..=21 {
                for card_count in [3, 4] {
                    scenarios.push(StrategyScenario {
                        situation: Situation {
                            kind: HandKind::Hard,
                            value,
                            dealer,
                            can_double: false,
                            can_split: false,
                            can_surrender: false,
                            split_aces: false,
                        },
                        card_count,
                    });
                }
            }
            for value in 13..=21 {
                for card_count in [3, 4] {
                    scenarios.push(StrategyScenario {
                        situation: Situation {
                            kind: HandKind::Soft,
                            value,
                            dealer,
                            can_double: false,
                            can_split: false,
                            can_surrender: false,
                            split_aces: false,
                        },
                        card_count,
                    });
                }
            }
        }
    }
    scenarios
}

#[derive(Debug, Clone)]
pub struct BasicStrategyDrill {
    pub queue: Vec<StrategyScenario>,
    pub completed: usize,
    pub include_multi_card: bool,
}

impl BasicStrategyDrill {
    pub fn new(seed: u64, include_multi_card: bool) -> Self {
        let mut queue = basic_strategy_catalog(include_multi_card);
        queue.shuffle(&mut SmallRng::seed_from_u64(seed));
        Self {
            queue,
            completed: 0,
            include_multi_card,
        }
    }

    pub fn current(&self) -> Option<StrategyScenario> {
        self.queue.get(self.completed).copied()
    }

    pub fn submit(&mut self, chosen: Action) -> Option<DrillFeedback> {
        let scenario = self.current()?;
        let expected = strategy::recommendation(scenario.situation).action;
        let feedback = DrillFeedback {
            chosen,
            expected,
            correct: chosen == expected,
            source: StrategySource::BasicStrategy,
        };
        self.completed += 1;
        Some(feedback)
    }

    pub fn is_finished(&self) -> bool {
        self.completed >= self.queue.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IndexScenario {
    pub play: IndexPlay,
    pub situation: Situation,
}

#[derive(Debug, Clone)]
pub struct CoreIndexDrill {
    pub queue: Vec<IndexScenario>,
    pub completed: usize,
}

impl CoreIndexDrill {
    pub fn new(seed: u64) -> Self {
        let mut queue: Vec<_> = strategy::core_index_plays()
            .iter()
            .copied()
            .map(|play| IndexScenario {
                play,
                situation: Situation {
                    kind: play.kind,
                    value: play.value,
                    dealer: play.dealer,
                    can_double: true,
                    can_split: play.kind == HandKind::Pair,
                    can_surrender: play.surrender_only,
                    split_aces: false,
                },
            })
            .collect();
        queue.shuffle(&mut SmallRng::seed_from_u64(seed));
        Self {
            queue,
            completed: 0,
        }
    }

    pub fn current(&self) -> Option<IndexScenario> {
        self.queue.get(self.completed).copied()
    }

    pub fn submit(&mut self, chosen: Action, entered_index: f32) -> Option<DrillFeedback> {
        let scenario = self.current()?;
        let (recommendation, source) = strategy::graded_action(
            scenario.situation,
            Some(scenario.play.threshold),
            strategy::core_index_plays(),
        );
        let correct = chosen == recommendation.action
            && (entered_index - scenario.play.threshold).abs() <= f32::EPSILON;
        self.completed += 1;
        Some(DrillFeedback {
            chosen: if correct {
                recommendation.action
            } else {
                chosen
            },
            expected: recommendation.action,
            correct,
            source,
        })
    }

    pub fn is_finished(&self) -> bool {
        self.completed >= self.queue.len()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountCheckpoint {
    pub prompt: bool,
    pub final_count: bool,
}

/// Uses the number of completed rounds before the next round, as the UI does.
pub fn count_checkpoint(
    completed_rounds: usize,
    interval: Option<usize>,
    past_cut: bool,
    final_shoe: bool,
) -> CountCheckpoint {
    if !past_cut {
        let prompt =
            interval.is_some_and(|every| every > 0 && (completed_rounds + 1).is_multiple_of(every));
        return CountCheckpoint {
            prompt,
            final_count: false,
        };
    }
    CountCheckpoint {
        prompt: interval.is_none(),
        final_count: final_shoe && interval.is_none(),
    }
}

#[derive(Debug, Clone)]
pub struct RunningCountDrill {
    shoe: Vec<Card>,
    cursor: usize,
    discard: Vec<Card>,
    exposed: Vec<Card>,
    hole: Option<Card>,
    pub rounds: usize,
    pub checkpoint: CountCheckpoint,
    pub interval: Option<usize>,
}

impl RunningCountDrill {
    pub fn new(seed: u64, interval: Option<usize>) -> Self {
        let mut shoe = standard_shoe(6);
        shoe.shuffle(&mut SmallRng::seed_from_u64(seed));
        Self {
            shoe,
            cursor: 0,
            discard: Vec::new(),
            exposed: Vec::new(),
            hole: None,
            rounds: 0,
            checkpoint: CountCheckpoint {
                prompt: false,
                final_count: false,
            },
            interval,
        }
    }

    pub fn start_round(&mut self) -> Result<(), GameError> {
        if self.shoe.len().saturating_sub(self.cursor) < 4 {
            return Err(GameError::ShoeExhausted);
        }
        self.exposed.clear();
        self.hole = None;
        for _ in 0..3 {
            let card = self.draw().ok_or(GameError::ShoeExhausted)?;
            self.exposed.push(card);
        }
        self.hole = Some(self.draw().ok_or(GameError::ShoeExhausted)?);
        Ok(())
    }
    pub fn running_count(&self) -> i32 {
        running_count(&self.discard) + running_count(&self.exposed)
    }

    pub fn hidden_hole_count(&self) -> i32 {
        self.running_count()
    }

    pub fn reveal_hole(&mut self) -> Option<Card> {
        let hole = self.hole.take()?;
        self.exposed.push(hole);
        Some(hole)
    }

    pub fn finish_round(&mut self, final_shoe: bool) -> CountCheckpoint {
        self.reveal_hole();
        self.discard.append(&mut self.exposed);
        let checkpoint = count_checkpoint(self.rounds, self.interval, self.past_cut(), final_shoe);
        self.rounds += 1;
        self.checkpoint = checkpoint;
        checkpoint
    }

    pub fn exposed_cards(&self) -> usize {
        self.discard.len() + self.exposed.len() + usize::from(self.hole.is_some())
    }

    pub fn past_cut(&self) -> bool {
        let remaining = 6 * 52usize - self.discard.len();
        remaining < 52
    }

    fn draw(&mut self) -> Option<Card> {
        let card = self.shoe.get(self.cursor).copied()?;
        self.cursor += 1;
        Some(card)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrueCountInfo {
    pub running_count: i32,
    pub divisor: f64,
    pub raw_true_count: f64,
    pub cards: usize,
    pub discarded_decks: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrueCountFeedback {
    pub submitted: i32,
    pub expected: i32,
    pub correct: bool,
    pub within_quarter_deck: bool,
}

#[derive(Debug, Clone)]
pub struct TrueCountDrill {
    pub decks: usize,
    pub half_deck: bool,
    pub duration_secs: u64,
    pub angle: u8,
    pub cards: Vec<Card>,
    pub running_count: i32,
    pub feedback: Option<TrueCountFeedback>,
}

impl TrueCountDrill {
    pub fn new(seed: u64, decks: usize, half_deck: bool, duration_secs: u64) -> Self {
        let mut rng = SmallRng::seed_from_u64(seed);
        loop {
            let mut shoe = standard_shoe(decks);
            shoe.shuffle(&mut rng);
            let max = ((shoe.len() as f64 * 0.9).floor() as usize).max(2);
            let count = rng.random_range(2..=max.min(shoe.len()));
            let cards = shoe.into_iter().take(count).collect::<Vec<_>>();
            let running_count = running_count(&cards);
            let answer = truncated_true_count(running_count, decks, cards.len(), half_deck);
            if running_count != 0 && answer != 0 {
                return Self {
                    decks,
                    half_deck,
                    duration_secs,
                    angle: 1,
                    cards,
                    running_count,
                    feedback: None,
                };
            }
        }
    }

    pub fn answer(&self) -> i32 {
        truncated_true_count(
            self.running_count,
            self.decks,
            self.cards.len(),
            self.half_deck,
        )
    }

    pub fn alternate_answer(&self) -> Option<i32> {
        quarter_deck_alternate(
            self.decks,
            self.cards.len(),
            self.running_count,
            self.half_deck,
        )
    }

    pub fn submit(&mut self, submitted: i32) -> TrueCountFeedback {
        let expected = self.answer();
        let within_quarter_deck =
            self.alternate_answer() == Some(submitted) && submitted != expected;
        let feedback = TrueCountFeedback {
            submitted,
            expected,
            correct: submitted == expected || within_quarter_deck,
            within_quarter_deck,
        };
        self.feedback = Some(feedback);
        feedback
    }

    pub fn info(&self) -> TrueCountInfo {
        let divisor = true_count_divisor(self.decks, self.cards.len(), self.half_deck);
        TrueCountInfo {
            running_count: self.running_count,
            divisor,
            raw_true_count: if divisor == 0.0 {
                0.0
            } else {
                f64::from(self.running_count) / divisor
            },
            cards: self.cards.len(),
            discarded_decks: self.cards.len() as f64 / 52.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TablePractice {
    pub game: Game,
    pub settings: ToolSettings,
    pub rounds: usize,
    pub pending_count: bool,
    pub finished: bool,
}

impl TablePractice {
    pub fn new(seed: u64, settings: ToolSettings) -> Self {
        Self {
            game: Game::new(seed),
            settings,
            rounds: 0,
            pending_count: false,
            finished: false,
        }
    }

    pub fn deal(&mut self) -> Result<(), GameError> {
        if self.finished || self.pending_count {
            return Err(GameError::RoundInProgress);
        }
        self.game.deal()
    }

    pub fn act(&mut self, action: Action) -> Result<(), GameError> {
        self.game.act(action)?;
        if self.game.phase == Phase::Finished {
            let checkpoint = count_checkpoint(
                self.rounds,
                self.settings.count_every,
                self.past_cut(),
                false,
            );
            self.rounds += 1;
            self.pending_count = checkpoint.prompt;
        }
        Ok(())
    }
    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        let situation = self
            .game
            .situation()
            .ok_or(GameError::InvalidState("no suite decision is pending"))?;
        let true_count = truncated_true_count(
            self.game.running_count(),
            6,
            self.game.discarded_count(),
            self.settings.half_deck,
        );
        let (recommendation, source) = strategy::graded_action(
            situation,
            Some(true_count as f32),
            strategy::core_index_plays(),
        );
        self.act(action)?;
        Ok(DrillFeedback {
            chosen: action,
            expected: recommendation.action,
            correct: action == recommendation.action,
            source,
        })
    }

    pub fn expected_count(&self) -> i32 {
        self.game.running_count()
    }
    pub fn past_cut(&self) -> bool {
        self.game.past_cut() || self.game.remaining_cards() <= 68
    }

    pub fn submit_count(&mut self, submitted: i32) -> bool {
        if !self.pending_count {
            return false;
        }
        self.pending_count = false;
        submitted == self.expected_count()
    }

    pub fn end(&mut self) {
        self.finished = true;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssessmentReport {
    pub shoes_completed: u8,
    pub attempts: u32,
    pub correct: u32,
    pub passed: bool,
    pub early_exit: bool,
}

#[derive(Debug, Clone)]
pub struct AssessmentSession {
    pub table: TablePractice,
    pub shoes_completed: u8,
    pub shoe_limit: u8,
    pub attempts: u32,
    pub correct: u32,
    pub early_exit: bool,
    seed: u64,
}

impl AssessmentSession {
    pub fn new(seed: u64, settings: ToolSettings) -> Self {
        Self {
            table: TablePractice::new(seed, settings),
            shoes_completed: 0,
            shoe_limit: 3,
            attempts: 0,
            correct: 0,
            early_exit: false,
            seed,
        }
    }

    pub fn record_answer(&mut self, correct: bool) {
        self.attempts = self.attempts.saturating_add(1);
        self.correct = self.correct.saturating_add(u32::from(correct));
    }
    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        let feedback = self.table.act_and_grade(action)?;
        self.record_answer(feedback.correct);
        Ok(feedback)
    }

    /// Completes a shoe only after the table has reached its cut card.
    pub fn finish_shoe(&mut self) -> bool {
        if self.shoes_completed >= self.shoe_limit
            || self.table.pending_count
            || self.table.game.phase != Phase::Finished
            || !self.table.past_cut()
        {
            return false;
        }
        self.shoes_completed += 1;
        if self.shoes_completed < self.shoe_limit {
            self.seed = self.seed.wrapping_add(1);
            self.table = TablePractice::new(self.seed, self.table.settings.clone());
        }
        true
    }

    pub fn exit_early(&mut self) {
        self.early_exit = true;
    }

    pub fn report(&self) -> AssessmentReport {
        AssessmentReport {
            shoes_completed: self.shoes_completed,
            attempts: self.attempts,
            correct: self.correct,
            passed: !self.early_exit
                && self.shoes_completed == self.shoe_limit
                && self.attempts > 0
                && self.correct == self.attempts,
            early_exit: self.early_exit,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressurePrompt {
    Math { left: u8, right: u8 },
    Mispay { dealer_mispayed: bool },
    TimedDecision { limit_secs: u8, elapsed_secs: u8 },
    GhostChange { occupied: bool },
}

pub fn math_answer(prompt: PressurePrompt) -> Option<i32> {
    match prompt {
        PressurePrompt::Math { left, right } => Some(i32::from(left) + i32::from(right)),
        _ => None,
    }
}

pub fn pressure_answer(prompt: PressurePrompt, answer: i32) -> bool {
    match prompt {
        PressurePrompt::Math { left, right } => answer == i32::from(left) + i32::from(right),
        PressurePrompt::Mispay { dealer_mispayed } => answer == i32::from(dealer_mispayed),
        PressurePrompt::TimedDecision {
            limit_secs,
            elapsed_secs,
        } => elapsed_secs <= limit_secs,
        PressurePrompt::GhostChange { occupied } => answer == i32::from(occupied),
    }
}

pub fn standard_shoe(decks: usize) -> Vec<Card> {
    let mut shoe = Vec::with_capacity(decks * 52);
    for _ in 0..decks {
        for suit in Suit::ALL {
            for rank in Rank::ALL {
                shoe.push(Card { rank, suit });
            }
        }
    }
    shoe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_count_excludes_hole_until_reveal() {
        let mut drill = RunningCountDrill::new(7, Some(1));
        drill.start_round().unwrap();
        let before = drill.hidden_hole_count();
        let hole = drill.reveal_hole().expect("a four-card round has a hole");
        assert_eq!(
            drill.running_count() - before,
            i32::from(crate::counting::card_value(hole.rank))
        );
    }

    #[test]
    fn count_checkpoint_uses_previous_round_number() {
        assert!(count_checkpoint(4, Some(5), false, false).prompt);
        assert!(!count_checkpoint(0, Some(5), false, false).prompt);
        assert!(count_checkpoint(20, None, true, true).final_count);
    }

    #[test]
    fn core_indexes_obey_threshold_boundaries_and_fallback() {
        let situation = Situation {
            kind: HandKind::Hard,
            value: 16,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        };
        assert_eq!(
            strategy::graded_action(situation, Some(-0.1), strategy::core_index_plays())
                .0
                .action,
            Action::Hit
        );
        assert_eq!(
            strategy::graded_action(situation, Some(0.0), strategy::core_index_plays())
                .0
                .action,
            Action::Stand
        );
    }

    #[test]
    fn every_shipped_index_matches_at_threshold_and_falls_back_below() {
        for play in strategy::core_index_plays() {
            let situation = Situation {
                kind: play.kind,
                value: play.value,
                dealer: play.dealer,
                can_double: true,
                can_split: play.kind == HandKind::Pair,
                can_surrender: play.surrender_only,
                split_aces: false,
            };
            let (_, at_source) = strategy::graded_action(
                situation,
                Some(play.threshold),
                strategy::core_index_plays(),
            );
            assert_eq!(at_source, StrategySource::CoreIndex, "{play:?}");
            let (_, below_source) = strategy::graded_action(
                situation,
                Some(play.threshold - 0.01),
                strategy::core_index_plays(),
            );
            assert_eq!(below_source, StrategySource::BasicStrategy, "{play:?}");
        }
    }

    #[test]
    fn insurance_index_is_legal_and_basic_strategy_falls_back_below_threshold() {
        let situation = Situation {
            kind: HandKind::Insurance,
            value: 0,
            dealer: 11,
            can_double: false,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        };
        let (below, below_source) =
            strategy::graded_action(situation, Some(2.99), strategy::core_index_plays());
        assert_eq!(below.action, Action::DeclineInsurance);
        assert_eq!(below_source, StrategySource::BasicStrategy);
        let (at, at_source) =
            strategy::graded_action(situation, Some(3.0), strategy::core_index_plays());
        assert_eq!(at.action, Action::Insure);
        assert_eq!(at_source, StrategySource::CoreIndex);
        assert!(situation.allows(at.action));
    }

    #[test]
    fn true_count_accepts_only_documented_alternate() {
        let mut drill = TrueCountDrill::new(42, 2, false, 90);
        let expected = drill.answer();
        let feedback = drill.submit(expected);
        assert!(feedback.correct);
        assert_eq!(drill.info().cards, drill.cards.len());
    }

    fn complete_assessment_shoe(session: &mut AssessmentSession) {
        loop {
            if session.table.pending_count {
                let expected = session.table.expected_count();
                assert!(session.table.submit_count(expected));
                continue;
            }
            if matches!(session.table.game.phase, Phase::Ready | Phase::Finished) {
                if session.table.past_cut() {
                    break;
                }
                session.table.deal().unwrap();
                continue;
            }
            let situation = session.table.game.situation().unwrap();
            let action = if situation.kind == HandKind::Insurance {
                Action::DeclineInsurance
            } else {
                Action::Stand
            };
            session.table.act(action).unwrap();
        }
        assert!(session.finish_shoe());
    }

    #[test]
    fn assessment_requires_three_complete_shoes_and_zero_misses() {
        let mut session = AssessmentSession::new(1, ToolSettings::default());
        assert!(!session.finish_shoe());
        session.record_answer(true);
        complete_assessment_shoe(&mut session);
        complete_assessment_shoe(&mut session);
        assert!(!session.report().passed);
        complete_assessment_shoe(&mut session);
        assert!(session.report().passed);

        session.exit_early();
        assert!(!session.report().passed);
    }
    #[test]
    fn current_stats_reset_without_losing_lifetime() {
        let mut stats = SuiteStats::default();
        stats.record(GradeBucket::Count, true);
        stats.current_elapsed_secs = 12;
        stats.reset_current();
        assert_eq!(stats.attempts, 1);
        assert_eq!(stats.correct, 1);
        assert_eq!(stats.current_attempts, 0);
        assert_eq!(stats.current_correct, 0);
        assert_eq!(stats.current_elapsed_secs, 0);
        assert_eq!(stats.buckets[&GradeBucket::Count].correct, 1);
        assert!(stats.current_buckets.is_empty());
    }

    #[test]
    fn pressure_prompts_have_real_grading() {
        let prompt = PressurePrompt::Math { left: 7, right: 8 };
        assert_eq!(math_answer(prompt), Some(15));
        assert!(pressure_answer(prompt, 15));
        assert!(!pressure_answer(prompt, 14));
        assert!(pressure_answer(
            PressurePrompt::TimedDecision {
                limit_secs: 5,
                elapsed_secs: 5,
            },
            0
        ));
    }
}
