//! Offline training-suite state machines and grading.
//!
//! The renderer consumes these types but does not implement count arithmetic,
//! strategy grading, timing, reports, or persistence rules.

use std::collections::BTreeMap;

use rand::{Rng, SeedableRng, rngs::SmallRng, seq::SliceRandom};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    counting::{quarter_deck_alternate, running_count, true_count_divisor, truncated_true_count},
    game::{Game, GameError, HandStatus, Phase},
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
    /// Base delay between automatic Running Count table states in milliseconds.
    ///
    /// The stored values are `0..=2000` in 200 ms steps. The displayed speed
    /// label runs in the opposite direction: 800 ms is speed 6.
    pub dealing_delay_ms: u16,
    /// True Count session timeout in seconds.
    pub true_count_duration_secs: u64,
}

pub const DEALING_DELAY_STEP_MS: u16 = 200;
pub const MAX_DEALING_DELAY_MS: u16 = 2_000;

pub const fn dealing_speed_label(delay_ms: u16) -> u8 {
    let bounded = if delay_ms > MAX_DEALING_DELAY_MS {
        MAX_DEALING_DELAY_MS
    } else {
        delay_ms
    };
    let steps = bounded / DEALING_DELAY_STEP_MS;
    10 - steps as u8
}

pub const fn dealing_delay_for_speed(speed: u8) -> Option<u16> {
    if speed <= 10 {
        Some((10 - speed) as u16 * DEALING_DELAY_STEP_MS)
    } else {
        None
    }
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            true_count_decks: 2,
            half_deck: false,
            count_every: Some(1),
            dealing_delay_ms: 800,
            true_count_duration_secs: 90,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunningCountAdvance {
    ShowingCards,
    CountPrompt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunningCountFeedback {
    pub submitted: i32,
    pub expected: i32,
    pub correct: bool,
    pub final_count: bool,
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
        prompt: final_shoe || interval.is_none(),
        final_count: final_shoe,
    }
}

/// A non-interactive table drill: the local basic-strategy player and dealer
/// advance automatically while the learner watches the exposed cards.
pub struct RunningCountDrill {
    game: Game,
    frames: Vec<Game>,
    frame_index: usize,
    round_complete: bool,
    pub rounds: usize,
    pub checkpoint: CountCheckpoint,
    pub interval: Option<usize>,
}

impl RunningCountDrill {
    pub fn new(seed: u64, interval: Option<usize>) -> Self {
        Self {
            game: Game::new(seed),
            frames: Vec::new(),
            frame_index: 0,
            round_complete: false,
            rounds: 0,
            checkpoint: CountCheckpoint {
                prompt: false,
                final_count: false,
            },
            interval,
        }
    }

    pub fn start_round(&mut self) -> Result<(), GameError> {
        if self.checkpoint.prompt || self.checkpoint.final_count || !self.round_complete {
            if self.frames.is_empty() {
                // A fresh drill has no round to continue.
            } else {
                return Err(GameError::RoundInProgress);
            }
        }
        self.game.deal()?;
        let (game, frames) = build_running_count_round(self.game.clone())?;
        self.game = game;
        self.frames = frames;
        self.frame_index = 0;
        self.round_complete = false;
        self.checkpoint = CountCheckpoint {
            prompt: false,
            final_count: false,
        };
        Ok(())
    }

    pub fn advance(&mut self) -> Result<RunningCountAdvance, GameError> {
        if self.checkpoint.prompt {
            return Ok(RunningCountAdvance::CountPrompt);
        }
        if self.frames.is_empty() {
            return Err(GameError::InvalidState(
                "no Running Count round has started",
            ));
        }
        if self.round_complete {
            self.start_round()?;
            return Ok(RunningCountAdvance::ShowingCards);
        }
        if self.frame_index + 1 < self.frames.len() {
            self.frame_index += 1;
        }
        if self.frame_index + 1 == self.frames.len() {
            self.complete_round();
            if self.checkpoint.prompt {
                return Ok(RunningCountAdvance::CountPrompt);
            }
        }
        Ok(RunningCountAdvance::ShowingCards)
    }

    pub fn current_game(&self) -> &Game {
        self.frames.get(self.frame_index).unwrap_or(&self.game)
    }

    pub fn running_count(&self) -> i32 {
        self.current_game().running_count()
    }

    pub fn hole_hidden(&self) -> bool {
        let game = self.current_game();
        game.phase != Phase::Finished && game.dealer.len() > 1
    }

    pub fn exposed_cards(&self) -> usize {
        let game = self.current_game();
        let visible_dealer = if self.hole_hidden() {
            game.dealer.len().saturating_sub(1)
        } else {
            game.dealer.len()
        };
        let player_cards = game
            .hands
            .iter()
            .map(|hand| hand.cards.len())
            .sum::<usize>();
        if game.phase == Phase::Finished {
            game.discarded_count()
        } else {
            game.discarded_count() + visible_dealer + player_cards
        }
    }

    pub fn past_cut(&self) -> bool {
        self.game.past_cut()
    }

    pub fn submit_count(&mut self, submitted: i32) -> Option<RunningCountFeedback> {
        if !self.checkpoint.prompt {
            return None;
        }
        let expected = self.running_count();
        let feedback = RunningCountFeedback {
            submitted,
            expected,
            correct: submitted == expected,
            final_count: self.checkpoint.final_count,
        };
        self.checkpoint.prompt = false;
        Some(feedback)
    }

    fn complete_round(&mut self) {
        let past_cut = self.game.past_cut();
        self.checkpoint = count_checkpoint(self.rounds, self.interval, past_cut, past_cut);
        self.rounds += 1;
        self.round_complete = true;
    }
}

fn build_running_count_round(mut game: Game) -> Result<(Game, Vec<Game>), GameError> {
    let mut frames = initial_running_count_frames(&game);
    for _ in 0..64 {
        let Some(situation) = game.situation() else {
            break;
        };
        game.act(strategy::recommendation(situation).action)?;
        append_running_count_transition(&mut frames, &game);
    }
    if game.phase != Phase::Finished {
        return Err(GameError::InvalidState(
            "automatic Running Count play did not settle",
        ));
    }
    if frames
        .last()
        .is_none_or(|frame| frame.phase != Phase::Finished)
    {
        frames.push(game.clone());
    }
    Ok((game, frames))
}

fn initial_running_count_frames(game: &Game) -> Vec<Game> {
    let Some(hand) = game.hands.first() else {
        return vec![game.clone()];
    };
    if game.dealer.len() < 2 || hand.cards.len() < 2 {
        return vec![game.clone()];
    }
    let mut first = game.clone();
    first.dealer.truncate(1);
    first.hands[0].cards.truncate(1);
    first.hands[0].status = HandStatus::Playing;
    first.phase = if game.phase == Phase::Insurance {
        Phase::Insurance
    } else {
        Phase::Playing
    };
    first.result = None;

    let mut second = first.clone();
    second.hands[0].cards = hand.cards.clone();

    let mut hole_hidden = game.clone();
    hole_hidden.result = None;
    if game.phase == Phase::Finished {
        hole_hidden.phase = Phase::Playing;
        hole_hidden.hands[0].status = HandStatus::Playing;
    }
    vec![first, second, hole_hidden]
}

fn append_running_count_transition(frames: &mut Vec<Game>, after: &Game) {
    if after.phase != Phase::Finished {
        frames.push(after.clone());
        return;
    }
    let dealer_blackjack = after
        .result
        .as_ref()
        .is_some_and(|result| result.dealer_blackjack);
    if !dealer_blackjack && after.dealer.len() >= 2 {
        let mut hole_hidden = after.clone();
        hole_hidden.dealer.truncate(2);
        hole_hidden.phase = Phase::Playing;
        hole_hidden.result = None;
        frames.push(hole_hidden.clone());
        for dealer_len in 3..after.dealer.len() {
            let mut dealer_hit = hole_hidden.clone();
            dealer_hit.dealer = after.dealer[..dealer_len].to_vec();
            frames.push(dealer_hit);
        }
    }
    frames.push(after.clone());
}

/// Invalid configuration supplied to the True Count drill constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TrueCountError {
    #[error("True Count supports 2, 4, 6, or 8 decks.")]
    InvalidDeckCount,
    #[error("True Count duration must be greater than zero.")]
    InvalidDuration,
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
    pub fn new(
        seed: u64,
        decks: usize,
        half_deck: bool,
        duration_secs: u64,
    ) -> Result<Self, TrueCountError> {
        if !matches!(decks, 2 | 4 | 6 | 8) {
            return Err(TrueCountError::InvalidDeckCount);
        }
        if duration_secs == 0 {
            return Err(TrueCountError::InvalidDuration);
        }
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
                return Ok(Self {
                    decks,
                    half_deck,
                    duration_secs,
                    angle: 0,
                    cards,
                    running_count,
                    feedback: None,
                });
            }
        }
    }

    pub fn turn_angle(&mut self, direction: i8) {
        self.angle = match direction {
            -1 if self.angle == 0 => 2,
            -1 => self.angle - 1,
            1 => (self.angle + 1) % 3,
            _ => self.angle,
        };
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

/// The current state of the optional pressure-table ghost seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GhostSeat {
    pub occupied: bool,
    pub changed: bool,
    pub situation: Option<Situation>,
    pub action: Option<Action>,
}

/// A deterministic expected-versus-paid settlement mismatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mispay {
    pub expected_half_units: i32,
    pub paid_half_units: i32,
}

/// A table-practice session with state-backed distractions and event grading.
#[derive(Debug, Clone)]
pub struct PressureTable {
    pub table: TablePractice,
    pub ghost: GhostSeat,
    pub mispay: Option<Mispay>,
    seed: u64,
}

impl PressureTable {
    pub const DECISION_LIMIT_SECS: u8 = 5;

    pub fn new(seed: u64, settings: ToolSettings) -> Self {
        Self {
            table: TablePractice::new(seed, settings),
            ghost: GhostSeat {
                occupied: true,
                changed: false,
                situation: None,
                action: None,
            },
            mispay: None,
            seed,
        }
    }

    pub fn deal(&mut self) -> Result<(), GameError> {
        self.table.deal()?;
        self.ghost.changed = self.table.rounds > 0 && self.table.rounds.is_multiple_of(3);
        if self.ghost.changed {
            self.ghost.occupied = !self.ghost.occupied;
        }
        self.seed = self.seed.wrapping_add(1);
        if !self.ghost.occupied {
            self.ghost.situation = None;
            self.ghost.action = None;
            return Ok(());
        }
        let mut ghost_game = Game::new(self.seed);
        ghost_game.deal()?;
        self.ghost.situation = ghost_game.situation();
        self.ghost.action = self.ghost.situation.map(|situation| {
            if situation.kind == HandKind::Insurance {
                Action::DeclineInsurance
            } else {
                strategy::recommendation(situation).action
            }
        });
        Ok(())
    }

    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        let feedback = self.table.act_and_grade(action)?;
        if self.table.game.phase == Phase::Finished {
            self.mispay = self
                .table
                .game
                .result
                .as_ref()
                .filter(|result| self.table.rounds.is_multiple_of(5) && result.net_half_units != 0)
                .map(|result| Mispay {
                    expected_half_units: result.net_half_units,
                    paid_half_units: result.net_half_units
                        + if result.net_half_units > 0 { -1 } else { 1 },
                });
        }
        Ok(feedback)
    }

    pub fn resolve_mispay(&mut self) {
        self.mispay = None;
    }

    pub fn resolve_ghost_change(&mut self) {
        self.ghost.changed = false;
    }

    pub fn pending_count(&self) -> bool {
        self.table.pending_count
    }

    pub fn submit_count(&mut self, submitted: i32) -> bool {
        self.table.submit_count(submitted)
    }

    pub fn expected_count(&self) -> i32 {
        self.table.expected_count()
    }

    pub fn past_cut(&self) -> bool {
        self.table.past_cut()
    }
}

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

/// Build deterministic local card faces for a strategy situation.
pub fn representative_cards(situation: Situation, card_count: u8) -> ([Card; 4], usize, Card) {
    let count = usize::from(card_count.clamp(2, 4));
    let mut cards = [Card {
        rank: Rank::Two,
        suit: Suit::Hearts,
    }; 4];
    let player_suits = [Suit::Hearts, Suit::Clubs, Suit::Diamonds, Suit::Spades];
    let dealer = Card {
        rank: rank_for_value(situation.dealer),
        suit: Suit::Diamonds,
    };
    match situation.kind {
        HandKind::Pair => {
            let rank = rank_for_value(situation.value);
            cards[0] = Card {
                rank,
                suit: player_suits[0],
            };
            cards[1] = Card {
                rank,
                suit: player_suits[1],
            };
        }
        HandKind::Soft => {
            cards[0] = Card {
                rank: Rank::Ace,
                suit: player_suits[0],
            };
            cards[1] = Card {
                rank: rank_for_value(situation.value.saturating_sub(11).max(2)),
                suit: player_suits[1],
            };
            for (index, suit) in player_suits.iter().enumerate().skip(2).take(count - 2) {
                cards[index] = Card {
                    rank: Rank::Two,
                    suit: *suit,
                };
            }
        }
        HandKind::Hard | HandKind::Insurance => {
            let first = if situation.kind == HandKind::Insurance {
                10
            } else {
                situation.value.saturating_sub(2).min(10)
            };
            let second = situation.value.saturating_sub(first).clamp(2, 11);
            cards[0] = Card {
                rank: rank_for_value(first),
                suit: player_suits[0],
            };
            cards[1] = Card {
                rank: rank_for_value(second),
                suit: player_suits[1],
            };
            for (index, suit) in player_suits.iter().enumerate().skip(2).take(count - 2) {
                cards[index] = Card {
                    rank: Rank::Two,
                    suit: *suit,
                };
            }
        }
    }
    (cards, count, dealer)
}

fn rank_for_value(value: u8) -> Rank {
    match value {
        2 => Rank::Two,
        3 => Rank::Three,
        4 => Rank::Four,
        5 => Rank::Five,
        6 => Rank::Six,
        7 => Rank::Seven,
        8 => Rank::Eight,
        9 => Rank::Nine,
        10 => Rank::Ten,
        11 => Rank::Ace,
        _ => Rank::Two,
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
    fn running_count_automatically_plays_the_round_before_prompting() {
        let mut drill = RunningCountDrill::new(7, Some(1));
        drill.start_round().unwrap();

        let mut steps = 0;
        while !drill.checkpoint.prompt {
            assert!(matches!(
                drill.advance().unwrap(),
                RunningCountAdvance::ShowingCards | RunningCountAdvance::CountPrompt
            ));
            steps += 1;
            assert!(steps < 100, "automatic round did not settle");
        }

        let game = drill.current_game();
        assert_eq!(game.phase, Phase::Finished);
        assert!(game.result.is_some());
        assert!(game.dealer.len() >= 2);
        assert!(
            game.hands
                .iter()
                .all(|hand| matches!(hand.status, crate::game::HandStatus::Settled))
        );
        assert!(steps > 1, "the drill should show multiple automatic states");
        assert_eq!(drill.rounds, 1);
        assert!(!drill.hole_hidden());
    }
    #[test]
    fn running_count_frames_include_automatic_player_and_dealer_cards() {
        for seed in 0..128 {
            let mut drill = RunningCountDrill::new(seed, Some(1));
            drill.start_round().unwrap();
            let mut saw_player_hit = false;
            let mut saw_dealer_hit = false;
            for _ in 0..100 {
                let game = drill.current_game();
                if game.phase == Phase::Playing {
                    saw_player_hit |= game.hands.iter().any(|hand| hand.cards.len() > 2);
                    saw_dealer_hit |= game.dealer.len() > 2;
                }
                if drill.checkpoint.prompt {
                    break;
                }
                drill.advance().unwrap();
            }
            if saw_player_hit && saw_dealer_hit {
                return;
            }
        }
        panic!("seed sweep did not observe both automatic player and dealer hits");
    }

    #[test]
    fn running_count_feedback_reports_an_incorrect_answer() {
        let mut drill = RunningCountDrill::new(7, Some(1));
        drill.start_round().unwrap();
        while !drill.checkpoint.prompt {
            drill.advance().unwrap();
        }

        let expected = drill.running_count();
        let feedback = drill.submit_count(expected + 1).unwrap();
        assert_eq!(feedback.expected, expected);
        assert_eq!(feedback.submitted, expected + 1);
        assert!(!feedback.correct);
        assert!(!drill.checkpoint.prompt);
    }

    #[test]
    fn running_count_speed_labels_use_the_reference_delay_scale() {
        assert_eq!(ToolSettings::default().dealing_delay_ms, 800);
        assert_eq!(dealing_speed_label(800), 6);
        assert_eq!(dealing_delay_for_speed(6), Some(800));
        assert_eq!(dealing_speed_label(0), 10);
        assert_eq!(dealing_speed_label(2_000), 0);
        assert_eq!(dealing_delay_for_speed(11), None);
    }

    #[test]
    fn running_count_can_skip_non_checkpoint_rounds() {
        let mut drill = RunningCountDrill::new(7, Some(3));
        drill.start_round().unwrap();
        while !drill.checkpoint.prompt && drill.rounds < 1 {
            drill.advance().unwrap();
        }
        assert_eq!(drill.rounds, 1);
        assert!(!drill.checkpoint.prompt);
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
        let mut drill = TrueCountDrill::new(42, 2, false, 90).unwrap();
        let expected = drill.answer();
        let feedback = drill.submit(expected);
        assert!(feedback.correct);
        assert_eq!(drill.info().cards, drill.cards.len());
    }

    #[test]
    fn true_count_rejects_unsupported_inputs_and_rotates_tray_angle() {
        assert!(matches!(
            TrueCountDrill::new(42, 0, false, 90),
            Err(TrueCountError::InvalidDeckCount)
        ));
        assert!(matches!(
            TrueCountDrill::new(42, 2, false, 0),
            Err(TrueCountError::InvalidDuration)
        ));
        let mut drill = TrueCountDrill::new(42, 2, false, 90).unwrap();
        assert_eq!(drill.angle, 0);
        drill.turn_angle(-1);
        assert_eq!(drill.angle, 2);
        drill.turn_angle(1);
        assert_eq!(drill.angle, 0);
    }

    #[test]
    fn pressure_table_exposes_ghost_state_from_a_real_deal() {
        let mut table = PressureTable::new(11, ToolSettings::default());
        table.deal().unwrap();
        assert!(table.ghost.occupied);
        assert!(table.ghost.situation.is_some());
        assert!(table.ghost.action.is_some());
        assert_eq!(table.table.game.phase, Phase::Playing);
    }

    #[test]
    fn pressure_table_generates_a_payout_audit_from_settlement() {
        let mut table = PressureTable::new(11, ToolSettings::default());
        let mut found = false;
        for _ in 0..20 {
            table.deal().unwrap();
            while let Some(situation) = table.table.game.situation() {
                let action = if situation.kind == HandKind::Insurance {
                    Action::DeclineInsurance
                } else {
                    Action::Stand
                };
                table.act_and_grade(action).unwrap();
            }
            if table.mispay.is_some() {
                found = true;
                break;
            }
            if table.pending_count() {
                let expected = table.expected_count();
                assert!(table.submit_count(expected));
            }
        }
        assert!(found);
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
