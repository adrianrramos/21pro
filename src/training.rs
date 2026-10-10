//! Decision history, sample-aware statistics, and a deterministic SM-2-style queue.
//!
//! A correct answer is SM-2 quality 4; an incorrect answer is quality 2. Failures
//! relearn after ten minutes rather than SM-2's next day. This is not FSRS and
//! is not identical to current Anki. Only an elapsed review advances its schedule.
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::counting::CountingRecord;
use crate::model::{ASSESSMENT_ROUNDS, Action, HandKind, RULESET_ID, Situation};
use crate::strategy::recommendation;

pub const SCHEMA_VERSION: u32 = 2;
const DAY: i64 = 86_400;
const RELEARNING_SECONDS: i64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StudyMode {
    Table,
    Practice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attempt {
    pub situation: Situation,
    pub chosen: Action,
    pub expected: Action,
    pub at: i64,
    pub mode: StudyMode,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundRecord {
    pub at: i64,
    pub net_half_units: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    pub situation: Situation,
    pub interval_days: u32,
    pub ease: f32,
    pub repetitions: u32,
    pub lapses: u32,
    pub due_at: i64,
    pub last_review_at: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CellStats {
    pub attempts: u32,
    pub mistakes: u32,
}

impl CellStats {
    /// None means unobserved, not perfect performance.
    pub fn accuracy(self) -> Option<f32> {
        (self.attempts != 0).then(|| (self.attempts - self.mistakes) as f32 / self.attempts as f32)
    }

    fn record(&mut self, correct: bool) {
        self.attempts = self.attempts.saturating_add(1);
        if !correct {
            self.mistakes = self.mistakes.saturating_add(1);
        }
    }

    // Beta(1, 4) prior: five virtual observations at 20% error. This tempers
    // one-off mistakes, while untested confidence improves with repeated success.
    fn smoothed_error(self) -> f64 {
        (f64::from(self.mistakes) + 1.0) / (f64::from(self.attempts) + 5.0)
    }
}

#[derive(Debug, Clone)]
pub struct SkillSummary {
    pub situation: Situation,
    pub stats: CellStats,
    pub due_at: Option<i64>,
}

#[derive(Debug, Clone, Default)]
pub struct Analytics {
    pub total: CellStats,
    pub cells: BTreeMap<(HandKind, u8, u8), CellStats>,
    pub categories: BTreeMap<HandKind, CellStats>,
    /// Nonempty, consecutive blocks of up to 25 decisions; the last may be partial.
    pub trend: Vec<CellStats>,
    /// Smoothed-error ordering, not a claim of statistical significance.
    pub weakest: Vec<SkillSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub schema_version: u32,
    pub ruleset_id: String,
    pub rounds: Vec<RoundRecord>,
    pub attempts: Vec<Attempt>,
    // A vector keeps structured Situation keys JSON-compatible.
    pub reviews: Vec<ReviewState>,
    #[serde(default)]
    pub counting_history: Vec<CountingRecord>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ruleset_id: RULESET_ID.to_owned(),
            rounds: Vec::new(),
            attempts: Vec::new(),
            reviews: Vec::new(),
            counting_history: Vec::new(),
        }
    }
}

impl Profile {
    /// Record exactly once when a real decision is committed, never on feedback
    /// dismissal or a free retry. The caller must apply the chosen table action.
    ///
    /// Panics before modifying history if the situation/action is invalid or the
    /// timestamp is negative. The fixed void API makes this a programmer invariant;
    /// the UI must offer only legal actions.
    pub fn record_attempt(
        &mut self,
        situation: Situation,
        chosen: Action,
        at: i64,
        mode: StudyMode,
    ) {
        assert!(valid_situation(situation), "invalid learning situation");
        assert!(situation.allows(chosen), "illegal learning action");
        assert!(at >= 0, "negative decision timestamp");
        let expected = recommendation(situation).action;
        let correct = chosen == expected;
        self.attempts.push(Attempt {
            situation,
            chosen,
            expected,
            at,
            mode,
        });

        if let Some(review) = self
            .reviews
            .iter_mut()
            .find(|review| review.situation == situation)
        {
            if !correct {
                lapse(review, at);
            } else if at >= review.due_at {
                review.interval_days = match review.repetitions {
                    0 => 1,
                    1 => 6,
                    _ => ((f64::from(review.interval_days) * f64::from(review.ease)).round()
                        as u32)
                        .max(1),
                };
                review.repetitions = review.repetitions.saturating_add(1);
                review.last_review_at = at;
                review.due_at = at.saturating_add(i64::from(review.interval_days) * DAY);
                // Quality 4 has zero ease adjustment under classic SM-2.
            }
        } else if !correct {
            let mut review = ReviewState {
                situation,
                interval_days: 0,
                ease: 2.5,
                repetitions: 0,
                lapses: 0,
                due_at: at,
                last_review_at: at,
            };
            lapse(&mut review, at);
            self.reviews.push(review);
        }
    }

    /// Call once after settlement of an ORIGINAL table round. Split children do
    /// not count separately; practice never calls this method.
    pub fn record_round(&mut self, at: i64, net_half_units: i32) {
        assert!(at >= 0, "negative round timestamp");
        self.rounds.push(RoundRecord { at, net_half_units });
    }

    /// Records one completed correct counting trial.
    ///
    /// `completed_at` is a Unix timestamp in seconds.
    /// `duration_ms` is the elapsed trial duration in milliseconds.
    pub fn record_counting_trial(&mut self, completed_at: i64, duration_ms: u64) {
        self.counting_history.push(CountingRecord {
            completed_at,
            duration_ms,
        });
    }

    pub fn rounds_played(&self) -> usize {
        self.rounds.len()
    }

    pub fn assessment_unlocked(&self) -> bool {
        self.rounds_played() >= ASSESSMENT_ROUNDS
    }

    pub fn analytics(&self, mode: Option<StudyMode>) -> Analytics {
        let mut result = Analytics::default();
        let mut exact = BTreeMap::<Situation, CellStats>::new();
        let mut block = CellStats::default();
        for attempt in self
            .attempts
            .iter()
            .filter(|a| mode.is_none_or(|mode| a.mode == mode))
        {
            let correct = attempt.chosen == attempt.expected;
            let situation = attempt.situation;
            result.total.record(correct);
            result
                .cells
                .entry((situation.kind, situation.value, situation.dealer))
                .or_default()
                .record(correct);
            result
                .categories
                .entry(situation.kind)
                .or_default()
                .record(correct);
            exact.entry(situation).or_default().record(correct);
            block.record(correct);
            if block.attempts == 25 {
                result.trend.push(block);
                block = CellStats::default();
            }
        }
        if block.attempts > 0 {
            result.trend.push(block);
        }
        let due: BTreeMap<_, _> = self
            .reviews
            .iter()
            .map(|r| (r.situation, r.due_at))
            .collect();
        result.weakest = exact
            .into_iter()
            .map(|(situation, stats)| SkillSummary {
                situation,
                stats,
                due_at: due.get(&situation).copied(),
            })
            .collect();
        result.weakest.sort_by(|a, b| {
            b.stats
                .smoothed_error()
                .total_cmp(&a.stats.smoothed_error())
                .then_with(|| a.stats.attempts.cmp(&b.stats.attempts))
                .then_with(|| a.situation.cmp(&b.situation))
        });
        result
    }

    /// Oldest due reviews first, then sample-aware weak/least-confident observed
    /// situations. Never synthesizes an unobserved or illegal context. Exact
    /// contexts stay distinct even when their heatmap cell is shared.
    pub fn practice_queue(&self, now: i64, limit: usize) -> Vec<Situation> {
        if !self.assessment_unlocked() || limit == 0 {
            return Vec::new();
        }
        let mut due: Vec<_> = self.reviews.iter().filter(|r| r.due_at <= now).collect();
        due.sort_by_key(|r| (r.due_at, r.situation));
        let mut seen = BTreeSet::new();
        let mut queue = Vec::new();
        for situation in due.into_iter().map(|r| r.situation).chain(
            std::iter::once_with(|| self.analytics(None))
                .flat_map(|analytics| analytics.weakest.into_iter().map(|s| s.situation)),
        ) {
            if seen.insert(situation) {
                queue.push(situation);
                if queue.len() == limit {
                    break;
                }
            }
        }
        queue
    }

    pub fn due_count(&self, now: i64) -> usize {
        self.reviews
            .iter()
            .filter(|review| review.due_at <= now)
            .count()
    }
}

fn lapse(review: &mut ReviewState, at: i64) {
    review.interval_days = 0;
    review.repetitions = 0;
    review.lapses = review.lapses.saturating_add(1);
    // Classic SM-2 quality 2: 0.1 - 3 * (0.08 + 3 * 0.02) = -0.32.
    review.ease = (review.ease - 0.32).max(1.3);
    review.last_review_at = at;
    review.due_at = at.saturating_add(RELEARNING_SECONDS);
}

/// Structural checks for persisted decisions. The engine owns card-level legality.
pub(crate) fn valid_situation(s: Situation) -> bool {
    if s.kind == HandKind::Insurance {
        return s.value == 0
            && s.dealer == 11
            && !s.can_double
            && !s.can_split
            && !s.can_surrender
            && !s.split_aces;
    }
    if !(2..=11).contains(&s.dealer)
        || (s.can_split && s.kind != HandKind::Pair)
        || (s.can_surrender && !s.can_double)
    {
        return false;
    }
    if s.split_aces {
        return s.kind == HandKind::Pair && s.value == 11 && !s.can_double && !s.can_surrender;
    }
    if s.kind == HandKind::Pair && (!s.can_double || (!s.can_split && s.can_surrender)) {
        return false;
    }
    match s.kind {
        HandKind::Hard => (5..=19).contains(&s.value) || (s.value == 20 && !s.can_double),
        HandKind::Soft => (13..=20).contains(&s.value),
        HandKind::Pair => (2..=11).contains(&s.value),
        HandKind::Insurance => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hard(value: u8, dealer: u8) -> Situation {
        Situation {
            kind: HandKind::Hard,
            value,
            dealer,
            can_double: true,
            can_split: false,
            can_surrender: true,
            split_aces: false,
        }
    }

    fn answer(profile: &mut Profile, s: Situation, correct: bool, at: i64, mode: StudyMode) {
        let expected = recommendation(s).action;
        let chosen = if correct {
            expected
        } else if expected == Action::Stand {
            Action::Hit
        } else {
            Action::Stand
        };
        profile.record_attempt(s, chosen, at, mode);
    }

    fn unlock(profile: &mut Profile) {
        for i in 0..250 {
            profile.record_round(i, 0);
        }
    }

    #[test]
    fn original_round_boundary_excludes_decisions_and_practice() {
        let mut p = Profile::default();
        for i in 0..300 {
            answer(&mut p, hard(16, 10), true, i, StudyMode::Practice);
        }
        assert_eq!(p.rounds_played(), 0);
        for i in 0..249 {
            p.record_round(i, 0);
        }
        assert!(!p.assessment_unlocked());
        assert!(p.practice_queue(1000, 10).is_empty());
        p.record_round(250, 3);
        assert!(p.assessment_unlocked());
        assert_eq!(p.practice_queue(1000, 10), vec![hard(16, 10)]);
    }

    #[test]
    fn elapsed_reviews_early_correct_lapse_and_recovery() {
        let mut p = Profile::default();
        let s = hard(16, 10);
        answer(&mut p, s, true, 50, StudyMode::Table);
        assert!(p.reviews.is_empty());
        answer(&mut p, s, false, 100, StudyMode::Table);
        assert_eq!(p.reviews[0].due_at, 700);
        assert_eq!(p.reviews[0].lapses, 1);
        assert_eq!(p.due_count(699), 0);
        assert_eq!(p.due_count(700), 1);
        answer(&mut p, s, true, 699, StudyMode::Practice);
        assert_eq!(p.reviews[0].due_at, 700);
        assert_eq!(p.reviews[0].last_review_at, 100);
        assert_eq!(p.reviews[0].repetitions, 0);
        for expected_days in [1, 6, 13] {
            let due = p.reviews[0].due_at;
            answer(&mut p, s, true, due, StudyMode::Practice);
            assert_eq!(p.reviews[0].interval_days, expected_days);
            assert_eq!(p.reviews[0].due_at, due + i64::from(expected_days) * DAY);
        }
        let due = p.reviews[0].due_at;
        answer(&mut p, s, false, due - 10, StudyMode::Practice);
        assert_eq!(p.reviews[0].due_at, due + 590);
        assert_eq!(p.reviews[0].repetitions, 0);
        assert_eq!(p.reviews[0].lapses, 2);
        for i in 0..10 {
            answer(&mut p, s, false, due + 1000 + i, StudyMode::Practice);
        }
        assert_eq!(p.reviews[0].ease, 1.3);
        let due = p.reviews[0].due_at;
        answer(&mut p, s, true, due, StudyMode::Practice);
        assert_eq!(p.reviews[0].interval_days, 1);
        assert_eq!(p.reviews.len(), 1);
    }

    #[test]
    fn analytics_filter_empty_partial_blocks_and_tempered_ranking() {
        let mut p = Profile::default();
        assert_eq!(p.analytics(None).total.accuracy(), None);
        assert!(p.analytics(None).trend.is_empty());
        let one_off = hard(12, 2);
        let repeated = hard(16, 10);
        answer(&mut p, one_off, false, 0, StudyMode::Table);
        for i in 0..10 {
            answer(&mut p, repeated, i >= 6, i, StudyMode::Table);
        }
        for i in 0..26 {
            answer(&mut p, repeated, i != 25, i, StudyMode::Practice);
        }
        let table = p.analytics(Some(StudyMode::Table));
        assert_eq!(
            table.total,
            CellStats {
                attempts: 11,
                mistakes: 7
            }
        );
        assert_eq!(table.weakest[0].situation, repeated);
        assert_eq!(table.weakest[0].stats.attempts, 10);
        let practice = p.analytics(Some(StudyMode::Practice));
        assert_eq!(practice.total.accuracy(), Some(25.0 / 26.0));
        assert_eq!(
            practice.trend,
            vec![
                CellStats {
                    attempts: 25,
                    mistakes: 0
                },
                CellStats {
                    attempts: 1,
                    mistakes: 1
                },
            ]
        );
        assert_eq!(p.analytics(None).total.attempts, 37);
    }

    #[test]
    fn exact_context_queue_is_unique_due_first_and_heatmap_aggregates() {
        let mut p = Profile::default();
        unlock(&mut p);
        let original = hard(16, 10);
        let hit_hand = Situation {
            can_double: false,
            can_surrender: false,
            ..original
        };
        let weak = hard(12, 2);
        answer(&mut p, original, false, 100, StudyMode::Table);
        answer(&mut p, hit_hand, false, 0, StudyMode::Table);
        for i in 0..8 {
            answer(&mut p, weak, false, 1000 + i, StudyMode::Table);
        }
        assert_eq!(p.practice_queue(700, 10), vec![hit_hand, original, weak]);
        assert_eq!(p.practice_queue(700, 1), vec![hit_hand]);
        assert!(p.practice_queue(700, 0).is_empty());
        assert_eq!(
            p.analytics(None).cells[&(HandKind::Hard, 16, 10)].attempts,
            2
        );
        assert_eq!(p.analytics(None).weakest.len(), 3);
    }

    #[test]
    fn all_correct_queue_prefers_less_observed_situations() {
        let mut p = Profile::default();
        unlock(&mut p);
        for i in 0..10 {
            answer(&mut p, hard(16, 10), true, i, StudyMode::Table);
        }
        answer(&mut p, hard(12, 2), true, 100, StudyMode::Table);
        assert_eq!(p.practice_queue(100, 10), vec![hard(12, 2), hard(16, 10)]);
    }

    #[test]
    fn an_unobserved_mode_is_not_perfect_and_insurance_remains_separate() {
        let mut p = Profile::default();
        let insurance = Situation {
            kind: HandKind::Insurance,
            value: 0,
            dealer: 11,
            can_double: false,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        };
        p.record_attempt(insurance, Action::Insure, 100, StudyMode::Table);
        let practice = p.analytics(Some(StudyMode::Practice));
        assert_eq!(practice.total.accuracy(), None);
        assert!(practice.categories.is_empty());
        assert!(practice.cells.is_empty());
        assert!(practice.weakest.is_empty());
        assert_eq!(
            p.analytics(None).categories[&HandKind::Insurance].mistakes,
            1
        );
        assert_eq!(p.reviews[0].situation, insurance);
    }

    #[test]
    fn illegal_actions_do_not_enter_history() {
        let mut p = Profile::default();
        let s = Situation {
            can_double: false,
            can_surrender: false,
            ..hard(16, 10)
        };
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                p.record_attempt(s, Action::Double, 0, StudyMode::Table);
            }))
            .is_err()
        );
        assert!(p.attempts.is_empty());
        assert!(p.reviews.is_empty());
    }
    #[test]
    fn impossible_practice_contexts_are_rejected() {
        let ordinary_pair_without_double = Situation {
            kind: HandKind::Pair,
            value: 8,
            dealer: 10,
            can_double: false,
            can_split: false,
            can_surrender: false,
            split_aces: false,
        };
        assert!(!valid_situation(ordinary_pair_without_double));
        assert!(!valid_situation(Situation {
            kind: HandKind::Hard,
            value: 4,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: true,
            split_aces: false,
        }));
    }
}
