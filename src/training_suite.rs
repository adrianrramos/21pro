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
    strategy::{self, IndexComparison, IndexPlay, StrategySource},
};

pub const SUITE_RULESET_ID: &str = crate::model::RULESET_ID;
// Stop before Game's automatic shuffle so the learner can answer the final count.
const SUITE_CUT_CARD: usize = 68;

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
        for card_count in 2..=if include_multi_card { 4 } else { 2 } {
            for kind in [HandKind::Hard, HandKind::Soft] {
                let minimum = if kind == HandKind::Hard {
                    2 * card_count
                } else {
                    (10 + card_count).max(13)
                };
                for value in minimum..=20 {
                    // Opening 2,2 and ten pairs belong to the pair catalog.
                    if kind == HandKind::Hard && card_count == 2 && matches!(value, 4 | 20) {
                        continue;
                    }
                    scenarios.push(StrategyScenario {
                        situation: Situation {
                            kind,
                            value,
                            dealer,
                            can_double: card_count == 2,
                            can_split: false,
                            can_surrender: card_count == 2,
                            split_aces: false,
                        },
                        card_count,
                    });
                }
            }
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
        if !scenario.situation.allows(chosen) {
            return None;
        }
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
                    can_double: play.kind != HandKind::Insurance,
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

    pub fn submit(
        &mut self,
        chosen: Action,
        comparison: IndexComparison,
        entered_index: f32,
    ) -> Option<DrillFeedback> {
        let scenario = self.current()?;
        if !scenario.situation.allows(chosen) {
            return None;
        }
        let correct = chosen == scenario.play.action
            && comparison == scenario.play.comparison
            && entered_index == scenario.play.threshold;
        self.completed += 1;
        Some(DrillFeedback {
            chosen,
            expected: scenario.play.action,
            correct,
            source: StrategySource::CoreIndex,
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
        if self.round_complete && self.frames.is_empty() {
            self.start_round()?;
            return Ok(RunningCountAdvance::ShowingCards);
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

    /// Reports whether the current round still has cards to render.
    pub fn cards_visible(&self) -> bool {
        !self.frames.is_empty()
    }

    /// Clears the rendered round while preserving the settled shoe for the next deal.
    pub fn clear_cards(&mut self) {
        self.frames.clear();
        self.frame_index = 0;
    }
    /// Reports whether the current round has settled and is ready to clear.
    pub fn round_complete(&self) -> bool {
        self.round_complete
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
        self.game.remaining_cards() <= SUITE_CUT_CARD
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
        let past_cut = self.past_cut();
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
        let mut dealer_reveal = after.clone();
        dealer_reveal.dealer.truncate(2);
        dealer_reveal.result = None;
        frames.push(dealer_reveal.clone());
        for dealer_len in 3..after.dealer.len() {
            let mut dealer_hit = dealer_reveal.clone();
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

/// Self-paced table play with checkpoints, including the last count before shuffle.
#[derive(Debug, Clone)]
pub struct TablePractice {
    pub game: Game,
    pub settings: ToolSettings,
    pub rounds: usize,
    pub pending_count: bool,
    pub finished: bool,
    shoe_rounds: usize,
}

impl TablePractice {
    pub fn new(seed: u64, settings: ToolSettings) -> Self {
        Self {
            game: Game::new(seed),
            settings,
            rounds: 0,
            pending_count: false,
            finished: false,
            shoe_rounds: 0,
        }
    }

    pub fn deal(&mut self) -> Result<(), GameError> {
        if self.finished || self.pending_count {
            return Err(GameError::RoundInProgress);
        }
        let new_shoe = self.game.phase == Phase::Finished && self.past_cut();
        self.game.deal()?;
        if new_shoe {
            self.shoe_rounds = 0;
        }
        self.complete_round();
        Ok(())
    }

    pub fn act(&mut self, action: Action) -> Result<(), GameError> {
        if self.finished || self.pending_count {
            return Err(GameError::RoundInProgress);
        }
        self.game.act(action)?;
        self.complete_round();
        Ok(())
    }

    fn complete_round(&mut self) {
        if self.game.phase == Phase::Finished {
            let past_cut = self.past_cut();
            self.pending_count = count_checkpoint(
                self.shoe_rounds,
                self.settings.count_every,
                past_cut,
                past_cut,
            )
            .prompt;
            self.rounds += 1;
            self.shoe_rounds += 1;
        }
    }
    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        let situation = self
            .game
            .situation()
            .ok_or(GameError::InvalidState("no suite decision is pending"))?;
        let running_count = self.game.running_count();
        let discarded = self.game.discarded_count();
        let true_count = truncated_true_count(running_count, 6, discarded, self.settings.half_deck);
        let (recommendation, source) = strategy::graded_action(
            situation,
            Some(true_count as f32),
            strategy::core_index_plays(),
        );
        let alternate_match = if action == recommendation.action {
            None
        } else {
            quarter_deck_alternate(6, discarded, running_count, self.settings.half_deck)
                .map(|count| {
                    strategy::graded_action(
                        situation,
                        Some(count as f32),
                        strategy::core_index_plays(),
                    )
                })
                .filter(|(alternate, _)| action == alternate.action)
        };
        self.act(action)?;
        Ok(DrillFeedback {
            chosen: action,
            expected: alternate_match
                .map_or(recommendation.action, |(alternate, _)| alternate.action),
            correct: action == recommendation.action || alternate_match.is_some(),
            source: alternate_match.map_or(source, |(_, source)| source),
        })
    }

    pub fn expected_count(&self) -> i32 {
        self.game.running_count()
    }
    pub fn past_cut(&self) -> bool {
        self.game.remaining_cards() <= SUITE_CUT_CARD
    }

    pub fn submit_count(&mut self, submitted: i32) -> bool {
        if self.finished || !self.pending_count {
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

/// The actual settlement compared with the pressure table's displayed payment.
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
    rng: SmallRng,
    ghost_return_chance: f64,
    ghost_game: Option<Game>,
    math: Option<PressurePrompt>,
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
            rng: SmallRng::seed_from_u64(seed),
            ghost_return_chance: 1.0 / 15.0,
            ghost_game: None,
            math: None,
        }
    }

    pub fn deal(&mut self) -> Result<(), GameError> {
        if self.ghost.changed || self.mispay.is_some() || self.math.is_some() {
            return Err(GameError::RoundInProgress);
        }
        let first_round = self.table.rounds == 0;
        self.table.deal()?;
        if !first_round {
            self.ghost.changed = self.rng.random_bool(self.ghost_return_chance);
            if self.ghost.changed {
                self.ghost.occupied = !self.ghost.occupied;
                self.ghost_return_chance = 1.0 / 15.0;
            } else if !self.ghost.occupied {
                self.ghost_return_chance = (self.ghost_return_chance + 0.2).min(0.9);
            }
        }
        self.seed = self.seed.wrapping_add(1);
        self.ghost.situation = None;
        self.ghost.action = None;
        if self.ghost.occupied {
            // The independent side table never changes the human shoe or count.
            let mut game = self
                .ghost_game
                .take()
                .unwrap_or_else(|| Game::new(self.seed));
            game.deal()?;
            let mut showed_unusual = false;
            while let Some(mut situation) = game.situation() {
                situation.can_surrender = false;
                let basic = strategy::recommendation(situation).action;
                let action = ghost_action(situation, self.rng.random_bool(0.3));
                if !showed_unusual || action != basic {
                    self.ghost.situation = Some(situation);
                    self.ghost.action = Some(action);
                }
                showed_unusual |= action != basic;
                game.act(action)?;
            }
            self.ghost_game = Some(game);
        } else {
            self.ghost_game = None;
        }
        self.complete_round();
        Ok(())
    }

    /// The fully played, independent ghost side table, not the human shoe.
    pub fn ghost_game(&self) -> Option<&Game> {
        self.ghost_game.as_ref()
    }

    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        if self.ghost.changed {
            return Err(GameError::RoundInProgress);
        }
        let feedback = self.table.act_and_grade(action)?;
        self.complete_round();
        Ok(feedback)
    }

    fn complete_round(&mut self) {
        if let Some(result) = &self.table.game.result {
            let expected = result.net_half_units;
            let wrong = expected != 0 && self.rng.random_bool(1.0 / 20.0);
            self.mispay = Some(Mispay {
                expected_half_units: expected,
                paid_half_units: expected + if wrong { -expected.signum() } else { 0 },
            });
            self.math = if !self.table.past_cut() && self.rng.random_bool(0.8) {
                let left = self.rng.random_range(2..=9);
                let right = self.rng.random_range(2..=9);
                let operator = match self.rng.random_range(0..4) {
                    0 => MathOperator::Add,
                    1 => MathOperator::Subtract,
                    2 => MathOperator::Multiply,
                    _ => MathOperator::Divide,
                };
                Some(PressurePrompt::Math {
                    left: if operator == MathOperator::Divide {
                        left * right
                    } else {
                        left
                    },
                    right,
                    operator,
                })
            } else {
                None
            };
        }
    }

    /// Pending distractions are generated from domain state, not renderer scores.
    pub fn pending_prompt(&self) -> Option<PressurePrompt> {
        if self.table.finished {
            return None;
        }
        if self.ghost.changed {
            return Some(PressurePrompt::GhostChange {
                occupied: self.ghost.occupied,
            });
        }
        if self.table.pending_count || self.table.game.phase != Phase::Finished {
            return None;
        }
        self.mispay
            .map(|audit| PressurePrompt::Mispay {
                dealer_mispayed: audit.expected_half_units != audit.paid_half_units,
            })
            .or(self.math)
    }

    /// Grades and consumes only the currently pending real event.
    pub fn submit_pressure(&mut self, answer: i32) -> Option<(GradeBucket, bool)> {
        let prompt = self.pending_prompt()?;
        let correct = pressure_answer(prompt, answer);
        let bucket = match prompt {
            PressurePrompt::Mispay { .. } => {
                self.mispay = None;
                GradeBucket::Mispay
            }
            PressurePrompt::GhostChange { .. } => {
                self.ghost.changed = false;
                GradeBucket::Deviation
            }
            PressurePrompt::Math { .. } => {
                self.math = None;
                GradeBucket::Math
            }
            PressurePrompt::TimedDecision { .. } => GradeBucket::Timing,
        };
        Some((bucket, correct))
    }

    pub fn pending_count(&self) -> bool {
        self.table.pending_count && !self.ghost.changed
    }

    pub fn submit_count(&mut self, submitted: i32) -> bool {
        !self.ghost.changed && self.table.submit_count(submitted)
    }

    pub fn expected_count(&self) -> i32 {
        self.table.expected_count()
    }

    pub fn past_cut(&self) -> bool {
        self.table.past_cut()
    }
}

fn ghost_action(mut situation: Situation, unusual: bool) -> Action {
    if situation.kind == HandKind::Insurance {
        return Action::DeclineInsurance;
    }
    situation.can_surrender = false;
    let basic = strategy::recommendation(situation).action;
    if !unusual {
        return basic;
    }
    let hard_total = match situation.kind {
        HandKind::Hard => Some(situation.value),
        HandKind::Pair if situation.value != 11 => Some(situation.value * 2),
        _ => None,
    };
    if situation.can_split && !matches!(situation.value, 5 | 10) {
        Action::Split
    } else if situation.can_double && hard_total.is_some_and(|total| (9..=11).contains(&total)) {
        Action::Double
    } else if basic == Action::Hit
        && (hard_total.is_some_and(|total| total >= 12)
            || (situation.kind == HandKind::Soft && situation.value >= 17))
    {
        Action::Stand
    } else {
        basic
    }
}

/// Exact assessment outcome; an early exit never constitutes a pass.
pub struct AssessmentReport {
    pub shoes_completed: u8,
    pub attempts: u32,
    pub correct: u32,
    pub passed: bool,
    pub early_exit: bool,
}

/// Three complete local shoes with locked five-round checks and exact final counts.
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
    pub fn new(seed: u64, mut settings: ToolSettings) -> Self {
        settings.count_every = Some(5);
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

    fn record_answer(&mut self, correct: bool) {
        self.attempts = self.attempts.saturating_add(1);
        self.correct = self.correct.saturating_add(u32::from(correct));
    }

    /// Deals within the current shoe; completing a cut shoe is a separate intent.
    pub fn deal(&mut self) -> Result<(), GameError> {
        if self.early_exit || self.shoes_completed >= self.shoe_limit || self.table.past_cut() {
            return Err(GameError::RoundInProgress);
        }
        self.table.deal()
    }

    /// Grades a pending periodic or final count exactly once.
    pub fn submit_count(&mut self, submitted: i32) -> Option<bool> {
        if self.early_exit || self.table.finished || !self.table.pending_count {
            return None;
        }
        let correct = self.table.submit_count(submitted);
        self.record_answer(correct);
        Some(correct)
    }
    pub fn act_and_grade(&mut self, action: Action) -> Result<DrillFeedback, GameError> {
        let feedback = self.table.act_and_grade(action)?;
        self.record_answer(feedback.correct);
        Ok(feedback)
    }

    /// Completes a shoe only after the table has reached its cut card.
    pub fn finish_shoe(&mut self) -> bool {
        if self.early_exit
            || self.table.finished
            || self.shoes_completed >= self.shoe_limit
            || self.table.pending_count
            || self.table.game.phase != Phase::Finished
            || !self.table.past_cut()
        {
            return false;
        }
        self.shoes_completed += 1;
        if self.shoes_completed < self.shoe_limit {
            self.seed = self.seed.wrapping_add(1);
            self.table.game = Game::new(self.seed);
            self.table.shoe_rounds = 0;
        } else {
            self.table.end();
        }
        true
    }

    pub fn exit_early(&mut self) {
        self.early_exit = true;
        self.table.end();
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
pub enum MathOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl MathOperator {
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Subtract => "−",
            Self::Multiply => "×",
            Self::Divide => "÷",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PressurePrompt {
    Math {
        left: u8,
        right: u8,
        operator: MathOperator,
    },
    Mispay {
        dealer_mispayed: bool,
    },
    TimedDecision {
        limit_secs: u8,
        elapsed_secs: u8,
    },
    GhostChange {
        occupied: bool,
    },
}

pub fn math_answer(prompt: PressurePrompt) -> Option<i32> {
    match prompt {
        PressurePrompt::Math {
            left,
            right,
            operator,
        } => {
            let left = i32::from(left);
            let right = i32::from(right);
            match operator {
                MathOperator::Add => Some(left + right),
                MathOperator::Subtract => Some(left - right),
                MathOperator::Multiply => Some(left * right),
                MathOperator::Divide if right != 0 && left % right == 0 => Some(left / right),
                MathOperator::Divide => None,
            }
        }
        _ => None,
    }
}

pub fn pressure_answer(prompt: PressurePrompt, answer: i32) -> bool {
    match prompt {
        PressurePrompt::Math { .. } => math_answer(prompt) == Some(answer),
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
    let player_suits = [Suit::Hearts, Suit::Clubs, Suit::Diamonds, Suit::Spades];
    let mut cards = std::array::from_fn(|index| Card {
        rank: Rank::Two,
        suit: player_suits[index],
    });
    let dealer = Card {
        rank: rank_for_value(situation.dealer),
        suit: Suit::Diamonds,
    };
    match situation.kind {
        HandKind::Pair => {
            cards[0].rank = rank_for_value(situation.value);
            cards[1].rank = cards[0].rank;
        }
        HandKind::Soft => {
            cards[0].rank = Rank::Ace;
            let mut remaining = situation.value.saturating_sub(10 + count as u8);
            for card in &mut cards[1..count] {
                let extra = remaining.min(9);
                let value = 1 + extra;
                card.rank = if value == 1 {
                    Rank::Ace
                } else {
                    rank_for_value(value)
                };
                remaining -= extra;
            }
        }
        HandKind::Hard => {
            let mut remaining = situation.value.saturating_sub(2 * count as u8);
            for card in &mut cards[..count] {
                let extra = remaining.min(8);
                card.rank = rank_for_value(2 + extra);
                remaining -= extra;
            }
        }
        HandKind::Insurance => {
            cards[0].rank = Rank::Ten;
            cards[1].rank = Rank::Seven;
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
    fn running_count_reaches_the_final_prompt_before_the_shoe_can_shuffle() {
        let mut drill = RunningCountDrill::new(7, None);
        drill.start_round().unwrap();
        for _ in 0..1000 {
            drill.advance().unwrap();
            if drill.checkpoint.final_count {
                assert!(drill.checkpoint.prompt);
                assert!(drill.current_game().remaining_cards() <= SUITE_CUT_CARD);
                assert!(
                    drill
                        .submit_count(drill.running_count())
                        .unwrap()
                        .final_count
                );
                return;
            }
        }
        panic!("one-shoe Running Count did not finish");
    }

    #[test]
    fn strategy_catalog_cards_match_the_graded_total_family_and_legal_context() {
        for scenario in basic_strategy_catalog(true) {
            let (cards, count, dealer) =
                representative_cards(scenario.situation, scenario.card_count);
            let cards = &cards[..count];
            let value = crate::model::hand_value(cards);
            let pair = count == 2 && cards[0].rank.value() == cards[1].rank.value();
            let kind = if pair {
                HandKind::Pair
            } else if value.soft {
                HandKind::Soft
            } else {
                HandKind::Hard
            };
            assert_eq!(kind, scenario.situation.kind, "{scenario:?}");
            assert_eq!(
                if pair {
                    cards[0].rank.value()
                } else {
                    value.total
                },
                scenario.situation.value,
                "{scenario:?}"
            );
            assert!(value.total < 21, "settled hands have no decision");
            assert_eq!(dealer.rank.value(), scenario.situation.dealer);
            assert_eq!(scenario.situation.can_double, count == 2);
            assert_eq!(scenario.situation.can_surrender, count == 2);
            assert_eq!(scenario.situation.can_split, pair);
            assert!(
                scenario
                    .situation
                    .allows(strategy::recommendation(scenario.situation).action)
            );
        }
    }

    #[test]
    fn index_drill_requires_the_strict_comparator_as_well_as_action_and_threshold() {
        let mut drill = CoreIndexDrill::new(7);
        let position = drill
            .queue
            .iter()
            .position(|scenario| {
                scenario.play.kind == HandKind::Hard
                    && scenario.play.value == 13
                    && scenario.play.dealer == 2
            })
            .unwrap();
        drill.queue.swap(0, position);
        let mut wrong_comparator = drill.clone();
        let mut wrong_threshold = drill.clone();
        assert!(
            drill
                .submit(Action::Hit, IndexComparison::Less, -1.0)
                .unwrap()
                .correct
        );
        assert!(
            !wrong_comparator
                .submit(Action::Hit, IndexComparison::GreaterOrEqual, -1.0)
                .unwrap()
                .correct
        );
        assert!(
            !wrong_threshold
                .submit(Action::Hit, IndexComparison::Less, 0.0)
                .unwrap()
                .correct
        );
    }

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
                saw_player_hit |= game.hands.iter().any(|hand| hand.cards.len() > 2);
                if game.dealer.len() > 2 {
                    saw_dealer_hit = true;
                    assert!(
                        !drill.hole_hidden(),
                        "dealer hits must follow a visible hole reveal"
                    );
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

    fn natural_seed() -> u64 {
        (0..4096)
            .find(|seed| {
                let mut game = Game::new(*seed);
                game.deal().unwrap();
                game.phase == Phase::Finished
            })
            .expect("seed range contains an immediately settled natural")
    }

    fn table_action(table: &TablePractice) -> Action {
        let situation = table.game.situation().unwrap();
        let count = truncated_true_count(
            table.game.running_count(),
            6,
            table.game.discarded_count(),
            table.settings.half_deck,
        );
        strategy::graded_action(situation, Some(count as f32), strategy::core_index_plays())
            .0
            .action
    }

    #[test]
    fn table_natural_deal_counts_the_round_and_blocks_dealing_until_checked() {
        let mut table = TablePractice::new(natural_seed(), ToolSettings::default());
        table.deal().unwrap();
        assert_eq!(table.game.phase, Phase::Finished);
        assert_eq!(table.rounds, 1);
        assert!(table.pending_count);
        let count = table.expected_count();
        assert_eq!(table.deal(), Err(GameError::RoundInProgress));
        assert_eq!(table.expected_count(), count);
        assert!(table.submit_count(count));
        assert!(!table.submit_count(count));
        table.deal().unwrap();
    }

    #[test]
    fn table_requires_a_final_count_before_game_auto_shuffle() {
        for interval in [None, Some(usize::MAX)] {
            let settings = ToolSettings {
                count_every: interval,
                ..ToolSettings::default()
            };
            let mut table = TablePractice::new(11, settings);
            let mut reached_cut = false;
            for _ in 0..100 {
                table.deal().unwrap();
                while table.game.situation().is_some() {
                    table.act(table_action(&table)).unwrap();
                }
                if table.past_cut() {
                    reached_cut = true;
                    assert!(table.pending_count);
                    let count = table.expected_count();
                    assert_eq!(table.deal(), Err(GameError::RoundInProgress));
                    assert_eq!(table.expected_count(), count);
                    assert!(table.submit_count(count));
                    let rounds = table.rounds;
                    table.deal().unwrap();
                    assert!(table.game.remaining_cards() > 200);
                    assert!(table.rounds >= rounds);
                    break;
                }
                assert!(!table.pending_count);
            }
            assert!(reached_cut, "the final count must precede the next shuffle");
            assert!(table.rounds > 1);
        }
    }

    #[test]
    fn ended_table_rejects_actions_without_consuming_cards_or_checks() {
        let mut table = TablePractice::new(11, ToolSettings::default());
        table.deal().unwrap();
        let action = table_action(&table);
        let remaining = table.game.remaining_cards();
        table.end();
        assert_eq!(table.act(action), Err(GameError::RoundInProgress));
        assert_eq!(table.game.remaining_cards(), remaining);
        assert_eq!(table.deal(), Err(GameError::RoundInProgress));
    }

    #[test]
    fn table_accepts_a_play_supported_only_by_the_quarter_deck_estimate() {
        for seed in 0..256 {
            let mut table = TablePractice::new(seed, ToolSettings::default());
            for _ in 0..100 {
                table.deal().unwrap();
                while let Some(situation) = table.game.situation() {
                    let primary = table_action(&table);
                    if let Some(count) = quarter_deck_alternate(
                        6,
                        table.game.discarded_count(),
                        table.game.running_count(),
                        table.settings.half_deck,
                    ) {
                        let alternate = strategy::graded_action(
                            situation,
                            Some(count as f32),
                            strategy::core_index_plays(),
                        )
                        .0
                        .action;
                        if alternate != primary {
                            let feedback = table.act_and_grade(alternate).unwrap();
                            assert!(feedback.correct);
                            assert_eq!(feedback.expected, alternate);
                            return;
                        }
                    }
                    table.act(primary).unwrap();
                }
                if table.past_cut() {
                    break;
                }
                if table.pending_count {
                    assert!(table.submit_count(table.expected_count()));
                }
            }
        }
        panic!("seed range must contain an actual table index boundary");
    }

    fn settle_pressure_round(table: &mut PressureTable) {
        if table.ghost.changed {
            let answer = i32::from(table.ghost.occupied);
            assert_eq!(
                table.submit_pressure(answer),
                Some((GradeBucket::Deviation, true))
            );
        }
        while table.table.game.situation().is_some() {
            let action = table_action(&table.table);
            table.act_and_grade(action).unwrap();
        }
        if table.pending_count() {
            assert!(table.submit_count(table.expected_count()));
        }
    }

    #[test]
    fn pressure_natural_deal_produces_a_real_payout_audit_after_the_count() {
        let mut table = PressureTable::new(natural_seed(), ToolSettings::default());
        table.deal().unwrap();
        assert_eq!(table.table.rounds, 1);
        let audit = table.mispay.unwrap();
        assert_eq!(
            audit.expected_half_units,
            table.table.game.result.as_ref().unwrap().net_half_units
        );
        assert!(table.pending_prompt().is_none());
        assert!(table.submit_count(table.expected_count()));
        let expected = i32::from(audit.expected_half_units != audit.paid_half_units);
        assert_eq!(table.deal(), Err(GameError::RoundInProgress));
        assert_eq!(
            table.submit_pressure(1 - expected),
            Some((GradeBucket::Mispay, false))
        );
        assert!(table.mispay.is_none());
    }

    #[test]
    fn pressure_events_are_resolved_once_and_include_correct_and_incorrect_payouts() {
        let mut table = PressureTable::new(11, ToolSettings::default());
        let mut saw_correct_payout = false;
        let mut saw_mispay = false;
        let mut operators = [false; 4];
        for _ in 0..100 {
            table.deal().unwrap();
            settle_pressure_round(&mut table);
            let audit = table.mispay.unwrap();
            let wrong = audit.expected_half_units != audit.paid_half_units;
            saw_mispay |= wrong;
            saw_correct_payout |= !wrong;
            assert_eq!(
                table.submit_pressure(i32::from(wrong)),
                Some((GradeBucket::Mispay, true))
            );
            while let Some(prompt) = table.pending_prompt() {
                let answer = match prompt {
                    PressurePrompt::Math { operator, .. } => {
                        operators[match operator {
                            MathOperator::Add => 0,
                            MathOperator::Subtract => 1,
                            MathOperator::Multiply => 2,
                            MathOperator::Divide => 3,
                        }] = true;
                        math_answer(prompt).unwrap()
                    }
                    PressurePrompt::GhostChange { occupied } => i32::from(occupied),
                    _ => panic!("payout audit should already be resolved"),
                };
                assert!(table.submit_pressure(answer).unwrap().1);
            }
            assert_eq!(table.submit_pressure(0), None);
            if saw_correct_payout && saw_mispay && operators.iter().all(|seen| *seen) {
                return;
            }
        }
        panic!("deterministic rounds did not exercise all pressure events");
    }

    #[test]
    fn pressure_ghost_side_table_plays_legally_and_can_leave_and_return() {
        let mut table = PressureTable::new(11, ToolSettings::default());
        let mut saw_empty = false;
        let mut saw_return = false;
        let mut saw_unusual = false;
        for _ in 0..200 {
            table.deal().unwrap();
            if let Some(game) = table.ghost_game() {
                assert!(table.ghost.occupied);
                assert_eq!(game.phase, Phase::Finished);
                assert!(game.result.is_some());
                assert!(
                    game.hands
                        .iter()
                        .all(|hand| hand.status == HandStatus::Settled)
                );
            } else {
                assert!(!table.ghost.occupied);
                saw_empty = true;
            }
            if let (Some(situation), Some(action)) = (table.ghost.situation, table.ghost.action) {
                assert!(situation.allows(action));
                assert_ne!(action, Action::Surrender);
                assert_ne!(action, Action::Insure);
                saw_unusual |= action != strategy::recommendation(situation).action;
            }
            saw_return |= saw_empty && table.ghost.occupied;
            if table.ghost.changed {
                let remaining = table.table.game.remaining_cards();
                assert_eq!(table.deal(), Err(GameError::RoundInProgress));
                assert_eq!(table.table.game.remaining_cards(), remaining);
            }
            settle_pressure_round(&mut table);
            while let Some(prompt) = table.pending_prompt() {
                let answer = match prompt {
                    PressurePrompt::Mispay { dealer_mispayed } => i32::from(dealer_mispayed),
                    PressurePrompt::Math { .. } => math_answer(prompt).unwrap(),
                    _ => panic!("ghost change should be resolved before player acts"),
                };
                assert!(table.submit_pressure(answer).unwrap().1);
            }
            if saw_return && saw_unusual {
                return;
            }
        }
        panic!("seeded ghost side table must leave, return, and make unusual plays");
    }

    #[test]
    fn assessment_locks_five_round_checks_and_cannot_grade_them_twice() {
        let mut session = AssessmentSession::new(
            11,
            ToolSettings {
                count_every: None,
                ..ToolSettings::default()
            },
        );
        for round in 1..=5 {
            session.deal().unwrap();
            while session.table.game.situation().is_some() {
                let action = table_action(&session.table);
                session.act_and_grade(action).unwrap();
            }
            assert_eq!(session.table.rounds, round);
            assert_eq!(session.table.pending_count, round == 5);
        }
        let count = session.table.expected_count();
        let attempts = session.attempts;
        assert!(session.deal().is_err());
        assert_eq!(session.submit_count(count), Some(true));
        assert_eq!(session.submit_count(count), None);
        assert_eq!(session.attempts, attempts + 1);
    }

    fn complete_assessment_shoe(session: &mut AssessmentSession, miss_final_count: bool) {
        let mut reached_cut = false;
        for _ in 0..1000 {
            if session.table.pending_count {
                let final_count = session.table.past_cut();
                let expected = session.table.expected_count();
                assert!(!session.finish_shoe());
                let correct = session
                    .submit_count(expected + i32::from(final_count && miss_final_count))
                    .unwrap();
                assert_eq!(correct, !(final_count && miss_final_count));
                if final_count {
                    reached_cut = true;
                    break;
                }
            } else if session.table.game.situation().is_some() {
                let action = table_action(&session.table);
                assert!(session.act_and_grade(action).unwrap().correct);
            } else {
                session.deal().unwrap();
            }
        }
        assert!(reached_cut, "shoe must finish with a checked final count");
        assert!(session.finish_shoe());
    }

    #[test]
    fn assessment_requires_three_complete_shoes_and_zero_misses() {
        let mut session = AssessmentSession::new(1, ToolSettings::default());
        assert_eq!(session.table.settings.count_every, Some(5));
        assert!(!session.finish_shoe());
        complete_assessment_shoe(&mut session, false);
        let rounds = session.table.rounds;
        let attempts = session.attempts;
        complete_assessment_shoe(&mut session, false);
        assert!(session.table.rounds > rounds);
        assert!(session.attempts > attempts);
        assert!(!session.report().passed);
        complete_assessment_shoe(&mut session, false);
        let report = session.report();
        assert!(report.passed);
        assert_eq!(report.shoes_completed, 3);
        assert_eq!(report.attempts, report.correct);
        assert!(report.attempts > 3);
        assert!(!session.finish_shoe());
        assert!(session.deal().is_err());
        assert_eq!(session.submit_count(0), None);
    }

    #[test]
    fn assessment_final_count_miss_and_early_exit_cannot_pass() {
        let mut session = AssessmentSession::new(1, ToolSettings::default());
        complete_assessment_shoe(&mut session, false);
        complete_assessment_shoe(&mut session, false);
        complete_assessment_shoe(&mut session, true);
        let report = session.report();
        assert!(!report.passed);
        assert_eq!(report.correct + 1, report.attempts);

        let mut exited = AssessmentSession::new(11, ToolSettings::default());
        exited.deal().unwrap();
        exited.exit_early();
        assert!(exited.report().early_exit);
        assert!(!exited.report().passed);
        assert!(exited.deal().is_err());
        assert!(exited.act_and_grade(Action::Stand).is_err());
        assert!(!exited.finish_shoe());
        assert_eq!(exited.submit_count(0), None);
        assert_eq!(exited.report().attempts, 0);
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
        for (operator, left, right, expected) in [
            (MathOperator::Add, 7, 8, 15),
            (MathOperator::Subtract, 2, 9, -7),
            (MathOperator::Multiply, 7, 8, 56),
            (MathOperator::Divide, 56, 8, 7),
        ] {
            let prompt = PressurePrompt::Math {
                left,
                right,
                operator,
            };
            assert_eq!(math_answer(prompt), Some(expected));
            assert!(pressure_answer(prompt, expected));
            assert!(!pressure_answer(prompt, expected + 1));
        }
        for (left, right) in [(7, 2), (8, 0)] {
            let prompt = PressurePrompt::Math {
                left,
                right,
                operator: MathOperator::Divide,
            };
            assert_eq!(math_answer(prompt), None);
            assert!(!pressure_answer(prompt, 0));
        }
        assert!(pressure_answer(
            PressurePrompt::TimedDecision {
                limit_secs: 5,
                elapsed_secs: 5,
            },
            0
        ));
        assert!(!pressure_answer(
            PressurePrompt::TimedDecision {
                limit_secs: 5,
                elapsed_secs: 6,
            },
            1
        ));
    }
}
