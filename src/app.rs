//! Desktop orchestration: render from state, then apply at most one user command.
//! The game engine never depends on egui, and progress is saved after each decision.
#[cfg(any(test, feature = "dev-fixtures"))]
mod fixtures;
mod views;
mod widgets;
#[cfg(test)]
mod workflow_tests;

use eframe::egui;
use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use twenty_one_pro::{
    counting::{CountingRecord, CountingTrial, parse_submitted_count, sorted_history},
    game::{Game, GameError, Phase},
    model::{Action, HandKind, Situation},
    storage::{self, Store},
    strategy::{self, IndexComparison},
    training::{Analytics, CellStats, Profile, StudyMode},
    training_suite::{
        AssessmentSession, BasicStrategyDrill, CoreIndexDrill, DrillFeedback, GradeBucket,
        PressurePrompt, PressureTable, RunningCountAdvance, RunningCountDrill, TablePractice,
        ToolId, ToolSession, TrueCountDrill, dealing_delay_for_speed,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Table,
    Insights,
    Practice,
    Counting,
    TrainingSuite,
    Rules,
}

struct Feedback {
    situation: Situation,
    chosen: Action,
    expected: Action,
    explanation: &'static str,
}
impl Feedback {
    fn correct(&self) -> bool {
        self.chosen == self.expected
    }
}

struct CountingAssessment {
    submitted: i32,
    actual: i32,
}

impl CountingAssessment {
    fn correct(&self) -> bool {
        self.submitted == self.actual
    }
}

enum SuiteActivity {
    RunningCount(RunningCountDrill),
    TrueCount(TrueCountDrill),
    BasicStrategy(BasicStrategyDrill),
    CoreIndex(CoreIndexDrill),
    TablePractice(TablePractice),
    Assessment(AssessmentSession),
    PressureTable(Box<PressureTable>),
}
const RUNNING_COUNT_ROUND_CLEAR_PAUSE: Duration = Duration::from_secs(3);
const RUNNING_COUNT_CARD_SLIDE_DURATION: Duration = Duration::from_millis(280);

const fn comparison_label(comparison: IndexComparison) -> &'static str {
    match comparison {
        IndexComparison::Less => "<",
        IndexComparison::LessOrEqual => "≤",
        IndexComparison::Greater => ">",
        IndexComparison::GreaterOrEqual => "≥",
    }
}

enum Command {
    Deal,
    Act(Action),
    StartPractice(Vec<Situation>),
    NextPractice,
    StartCounting,
    NextCounting,
    FinishCounting,
    SubmitCounting,
    SuiteAct(Action),
    StartSuite(ToolId),
    SuiteSetCountEvery(Option<usize>),
    SuiteSetDealingSpeed(u8),
    SuiteTrueCountAngle(i8),
    SuiteAdjustInput(i32),
    SuiteSetComparison(IndexComparison),
    SuiteSetTrueCountDecks(usize),
    SuiteSetHalfDeck(bool),
    SuiteSetTrueCountDuration(u64),
    ResetSuiteCurrent,
    SuiteNext,
    SuiteRestart,
    SuiteSubmit,
    SuiteEnd,
    SuiteCompleteShoe,
    RetrySave,
}

pub struct TrainerApp {
    page: Page,
    profile: Profile,
    store: Option<Store>,
    data_path: PathBuf,
    startup_error: Option<String>,
    error: Option<String>,
    dirty: bool,
    allow_close: bool,
    close_warning: bool,
    table: Game,
    table_feedback: Vec<Feedback>,
    practice: Option<Game>,
    practice_feedback: Vec<Feedback>,
    practice_queue: VecDeque<Situation>,
    practice_total: usize,
    practice_completed: usize,
    practice_start_attempt: usize,
    practice_target: Option<Situation>,
    counting: Option<CountingTrial>,
    counting_started_at: Option<Instant>,
    counting_elapsed: Option<Duration>,
    counting_input: String,
    counting_input_error: Option<String>,
    counting_assessment: Option<CountingAssessment>,
    suite: Option<SuiteActivity>,
    suite_input: String,
    suite_selected_action: Option<Action>,
    suite_comparison: IndexComparison,
    suite_feedback: Option<DrillFeedback>,
    suite_message: Option<String>,
    suite_report: Option<ToolSession>,
    suite_pressure_prompt: Option<PressurePrompt>,
    suite_pressure_decision_started_at: Option<Instant>,
    suite_running_next_step_at: Option<Instant>,
    suite_running_clear_at: Option<Instant>,
    suite_running_step_started_at: Option<Instant>,
    suite_started_at: Option<Instant>,
    suite_elapsed: Duration,
    suite_seed: u64,
    suite_attempts: u32,
    suite_correct: u32,
    suite_mistakes: BTreeMap<twenty_one_pro::training_suite::GradeBucket, u32>,
    // Sorted committed history; pending writes stay out until retry succeeds.
    counting_history: Vec<CountingRecord>,
    filter: Option<StudyMode>,
    analytics: Analytics,
    table_stats: CellStats,
    heatmap: HandKind,
    selected_cell: Option<(HandKind, u8, u8)>,
    just_unlocked: bool,
    // Store is declared first so the database closes before its temporary directory.
    #[cfg(any(test, feature = "dev-fixtures"))]
    fixture: Option<fixtures::Session>,
}

fn configured_data_path(directory: Option<OsString>) -> Option<PathBuf> {
    directory
        .filter(|directory| !directory.is_empty())
        .map(|directory| PathBuf::from(directory).join("profile.redb"))
}

impl TrainerApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        #[cfg(feature = "dev-fixtures")]
        if let Some(fixture) = fixtures::Fixture::from_env()? {
            return Self::fixture(&cc.egui_ctx, fixture);
        }
        #[cfg(not(feature = "dev-fixtures"))]
        if std::env::var_os("TWENTY_ONE_PRO_FIXTURE").is_some() {
            return Err("TWENTY_ONE_PRO_FIXTURE requires the dev-fixtures feature".into());
        }
        let mut startup_error = None;
        let mut store = None;
        let mut profile = Profile::default();
        let path = match configured_data_path(std::env::var_os("TWENTY_ONE_PRO_DATA_DIR")) {
            Some(path) => Ok(path),
            None => storage::default_path(),
        };
        let data_path = match path {
            Ok(path) => {
                match Store::open(&path).and_then(|db| db.load().map(|saved| (db, saved))) {
                    Ok((db, saved)) => {
                        store = Some(db);
                        profile = saved;
                    }
                    Err(error) => startup_error = Some(error.to_string()),
                }
                path
            }
            Err(error) => {
                startup_error = Some(error.to_string());
                PathBuf::new()
            }
        };
        let mut app = Self::with_profile(
            &cc.egui_ctx,
            profile,
            store,
            data_path,
            Game::new(rand::random()),
        );
        app.startup_error = startup_error;
        Ok(app)
    }

    fn with_profile(
        ctx: &egui::Context,
        profile: Profile,
        store: Option<Store>,
        data_path: PathBuf,
        table: Game,
    ) -> Self {
        widgets::configure(ctx);
        let analytics = profile.analytics(Some(StudyMode::Table));
        let table_stats = analytics.total;
        let counting_history = sorted_history(&profile.counting_history);
        Self {
            page: Page::Table,
            profile,
            store,
            data_path,
            startup_error: None,
            error: None,
            dirty: false,
            allow_close: false,
            close_warning: false,
            table,
            table_feedback: Vec::new(),
            practice: None,
            practice_feedback: Vec::new(),
            practice_queue: VecDeque::new(),
            practice_total: 0,
            practice_completed: 0,
            practice_start_attempt: 0,
            practice_target: None,
            counting: None,
            counting_started_at: None,
            counting_elapsed: None,
            counting_input: String::new(),
            counting_input_error: None,
            counting_assessment: None,
            suite: None,
            suite_input: String::new(),
            suite_selected_action: None,
            suite_comparison: IndexComparison::GreaterOrEqual,
            suite_feedback: None,
            suite_message: None,
            suite_report: None,
            suite_pressure_prompt: None,
            suite_pressure_decision_started_at: None,
            suite_running_next_step_at: None,
            suite_running_clear_at: None,
            suite_running_step_started_at: None,
            suite_started_at: None,
            suite_elapsed: Duration::ZERO,
            suite_seed: rand::random(),
            suite_attempts: 0,
            suite_correct: 0,
            suite_mistakes: BTreeMap::new(),
            counting_history,
            filter: Some(StudyMode::Table),
            analytics,
            table_stats,
            heatmap: HandKind::Hard,
            selected_cell: None,
            just_unlocked: false,
            #[cfg(any(test, feature = "dev-fixtures"))]
            fixture: None,
        }
    }

    fn counting_duration(&self) -> Duration {
        self.counting_elapsed
            .or_else(|| self.counting_started_at.map(|started| started.elapsed()))
            .unwrap_or_default()
    }

    fn cache_counting_history(&mut self) {
        while self.counting_history.len() < self.profile.counting_history.len() {
            let record = self.profile.counting_history[self.counting_history.len()];
            let key = (record.duration_ms, record.completed_at);
            let position = self
                .counting_history
                .binary_search_by_key(&key, |cached| (cached.duration_ms, cached.completed_at))
                .unwrap_or_else(|position| position);
            self.counting_history.insert(position, record);
        }
    }

    fn reset_counting_trial(&mut self) {
        self.reset_counting_trial_with_seed(rand::random());
    }

    fn reset_counting_trial_with_seed(&mut self, seed: u64) {
        let mut trial = CountingTrial::new(seed);
        if let Err(error) = trial.start() {
            self.error = Some(format!("Cannot start card-counting trial: {error}"));
            return;
        }
        self.counting = Some(trial);
        self.counting_started_at = Some(Instant::now());
        self.counting_elapsed = None;
        self.counting_input.clear();
        self.counting_input_error = None;
        self.counting_assessment = None;
        self.error = None;
    }

    fn now(&self) -> Result<i64, String> {
        #[cfg(any(test, feature = "dev-fixtures"))]
        if self.fixture.is_some() {
            return Ok(fixtures::NOW);
        }
        Self::unix_timestamp(SystemTime::now())
    }

    fn unix_timestamp(time: SystemTime) -> Result<i64, String> {
        time.duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
            .map_err(|error| format!("System clock is before the Unix epoch: {error}"))
    }

    fn practice_seed(&mut self) -> u64 {
        #[cfg(any(test, feature = "dev-fixtures"))]
        if let Some(fixture) = &mut self.fixture {
            return fixture.seed();
        }
        rand::random()
    }

    fn data_path_label(&self) -> std::borrow::Cow<'_, str> {
        #[cfg(any(test, feature = "dev-fixtures"))]
        if self.fixture.is_some() {
            return "Temporary visual fixture profile (discarded on exit)".into();
        }
        self.data_path.to_string_lossy()
    }

    fn save_status(&self) -> &'static str {
        if self.dirty {
            return "UNSAVED CHANGES";
        }
        #[cfg(any(test, feature = "dev-fixtures"))]
        if self.fixture.is_some() {
            return "VISUAL FIXTURE · TEMPORARY";
        }
        "PROGRESS SAVED LOCALLY"
    }

    fn refresh_analytics(&mut self) {
        self.analytics = self.profile.analytics(self.filter);
        self.table_stats = if self.filter == Some(StudyMode::Table) {
            self.analytics.total
        } else {
            self.profile.analytics(Some(StudyMode::Table)).total
        };
    }

    fn persist_progress(&mut self) -> bool {
        self.dirty = true;
        let Some(store) = &self.store else {
            return false;
        };
        match store.save(&self.profile) {
            Ok(()) => {
                self.dirty = false;
                self.error = None;
                self.close_warning = false;
                true
            }
            Err(error) => {
                self.error = Some(format!("Progress could not be saved: {error}"));
                false
            }
        }
    }

    // Called once per successful deal/action; finished games reject further actions.
    fn finish_round(&mut self, practice: bool, at: i64) {
        if practice {
            if let Some(game) = &self.practice
                && game.phase == Phase::Finished
            {
                self.practice_completed += 1;
            }
        } else if let Some(result) = &self.table.result {
            let was_unlocked = self.profile.assessment_unlocked();
            self.profile.record_round(at, result.net_half_units);
            if !was_unlocked && self.profile.assessment_unlocked() {
                self.just_unlocked = true;
                self.page = Page::Insights;
            }
        }
    }

    fn start_next_practice(&mut self) {
        if let Some(target) = self.practice_queue.front().copied() {
            match Game::practice(target, self.practice_seed()) {
                Ok(game) => {
                    self.practice_queue.pop_front();
                    self.practice = Some(game);
                    self.practice_target = Some(target);
                    self.practice_feedback.clear();
                    self.error = None;
                }
                Err(error) => {
                    self.error = Some(format!("Cannot create this practice hand: {error}"))
                }
            }
        } else {
            self.practice = None;
            self.practice_target = None;
        }
    }
    fn running_count_step_delay(&self) -> Duration {
        Duration::from_millis(
            u64::from(self.profile.suite_settings.dealing_delay_ms).saturating_add(80),
        )
    }

    fn start_suite(&mut self, tool: ToolId) {
        let seed = self.suite_seed;
        self.suite_seed = self.suite_seed.wrapping_add(1);
        self.suite_input = "0".to_owned();
        self.suite_selected_action = None;
        self.suite_comparison = IndexComparison::GreaterOrEqual;
        self.suite_feedback = None;
        self.suite_message = None;
        self.suite_report = None;
        self.suite_pressure_prompt = None;
        self.suite_pressure_decision_started_at = None;
        self.suite_running_next_step_at = None;
        self.suite_running_clear_at = None;
        self.suite_running_step_started_at = None;
        self.suite_elapsed = Duration::ZERO;
        self.suite_attempts = 0;
        self.suite_correct = 0;
        self.suite_mistakes.clear();
        let started_at = Instant::now();
        self.suite_started_at = Some(started_at);
        let settings = self.profile.suite_settings.clone();
        let activity = match tool {
            ToolId::RunningCount => {
                let mut drill = RunningCountDrill::new(seed, settings.count_every);
                if let Err(error) = drill.start_round() {
                    self.suite_started_at = None;
                    self.error = Some(format!("Cannot start Running Count drill: {error}"));
                    return;
                }
                SuiteActivity::RunningCount(drill)
            }
            ToolId::TrueCount => match TrueCountDrill::new(
                seed,
                settings.true_count_decks,
                settings.half_deck,
                settings.true_count_duration_secs,
            ) {
                Ok(drill) => SuiteActivity::TrueCount(drill),
                Err(error) => {
                    self.suite_started_at = None;
                    self.error = Some(format!("Cannot start True Count drill: {error}"));
                    return;
                }
            },
            ToolId::BasicStrategy => {
                SuiteActivity::BasicStrategy(BasicStrategyDrill::new(seed, true))
            }
            ToolId::CoreIndexPlays => SuiteActivity::CoreIndex(CoreIndexDrill::new(seed)),
            ToolId::TablePractice => {
                SuiteActivity::TablePractice(TablePractice::new(seed, settings.clone()))
            }
            ToolId::Assessment => {
                SuiteActivity::Assessment(AssessmentSession::new(seed, settings.clone()))
            }
            ToolId::PressureTable => {
                SuiteActivity::PressureTable(Box::new(PressureTable::new(seed, settings)))
            }
        };
        self.suite = Some(activity);
        if matches!(tool, ToolId::RunningCount) {
            self.suite_running_next_step_at = Some(started_at + self.running_count_step_delay());
            self.suite_running_step_started_at = Some(started_at);
        }
        self.page = Page::TrainingSuite;
    }

    fn suite_record(&mut self, bucket: GradeBucket, correct: bool) {
        self.suite_attempts = self.suite_attempts.saturating_add(1);
        self.suite_correct = self.suite_correct.saturating_add(u32::from(correct));
        if !correct {
            *self.suite_mistakes.entry(bucket).or_default() += 1;
        }
        self.profile.record_suite_attempt(bucket, correct);
        self.persist_progress();
    }
    fn begin_pressure_prompt(&mut self) {
        let Some(SuiteActivity::PressureTable(table)) = self.suite.as_ref() else {
            return;
        };
        self.suite_pressure_prompt = table.pending_prompt();
        if self.suite_pressure_prompt.is_some() || table.table.game.situation().is_none() {
            self.suite_pressure_decision_started_at = None;
        } else if self.suite_pressure_decision_started_at.is_none() {
            self.suite_pressure_decision_started_at = Some(Instant::now());
        }
    }
    fn tick_running_count(&mut self, now: Instant) {
        if let Some(clear_at) = self.suite_running_clear_at {
            if now < clear_at {
                return;
            }
            if let Some(SuiteActivity::RunningCount(drill)) = self.suite.as_mut() {
                drill.clear_cards();
            } else {
                self.suite_running_clear_at = None;
                self.suite_running_next_step_at = None;
                return;
            }
            self.suite_running_clear_at = None;
            self.suite_running_step_started_at = Some(now);
            self.suite_running_next_step_at = Some(now + self.running_count_step_delay());
            return;
        }

        let Some(next_step) = self.suite_running_next_step_at else {
            return;
        };
        if now < next_step {
            return;
        }
        let outcome = match self.suite.as_mut() {
            Some(SuiteActivity::RunningCount(drill)) => drill.advance(),
            _ => {
                self.suite_running_next_step_at = None;
                self.suite_running_step_started_at = None;
                return;
            }
        };
        match outcome {
            Ok(RunningCountAdvance::CountPrompt) => {
                self.suite_running_next_step_at = None;
                self.suite_running_clear_at = Some(now + RUNNING_COUNT_ROUND_CLEAR_PAUSE);
                self.suite_running_step_started_at = Some(now);
                self.suite_input = "0".to_owned();
                self.suite_message = None;
            }
            Ok(RunningCountAdvance::ShowingCards) => {
                self.suite_running_step_started_at = Some(now);
                let round_complete = matches!(
                    self.suite.as_ref(),
                    Some(SuiteActivity::RunningCount(drill)) if drill.round_complete()
                );
                if round_complete {
                    self.suite_running_next_step_at = None;
                    self.suite_running_clear_at = Some(now + RUNNING_COUNT_ROUND_CLEAR_PAUSE);
                } else {
                    self.suite_running_next_step_at = Some(now + self.running_count_step_delay());
                }
            }
            Err(error) => {
                self.suite_running_next_step_at = None;
                self.suite_running_step_started_at = None;
                self.error = Some(format!("Running Count could not advance: {error}"));
            }
        }
    }

    fn running_count_card_progress(&self, now: Instant) -> f32 {
        let Some(started_at) = self.suite_running_step_started_at else {
            return 1.0;
        };
        let elapsed = now.checked_duration_since(started_at).unwrap_or_default();
        (elapsed.as_secs_f32() / RUNNING_COUNT_CARD_SLIDE_DURATION.as_secs_f32()).min(1.0)
    }

    fn suite_finish(&mut self, tool: ToolId, finished: bool, passed: Option<bool>) {
        let elapsed = self
            .suite_started_at
            .take()
            .map_or(self.suite_elapsed, |started| started.elapsed());
        self.suite_elapsed = elapsed;
        self.suite_pressure_decision_started_at = None;
        self.suite_running_next_step_at = None;
        self.suite_running_clear_at = None;
        self.suite_running_step_started_at = None;
        let session = ToolSession {
            tool,
            elapsed_secs: elapsed.as_secs(),
            finished,
            passed,
            attempts: self.suite_attempts,
            correct: self.suite_correct,
            mistakes: self.suite_mistakes.clone(),
        };
        self.profile.record_suite_session(session.clone());
        self.suite_report = finished.then_some(session);
        self.persist_progress();
    }

    fn execute(&mut self, command: Command) {
        if matches!(command, Command::RetrySave) {
            if self.persist_progress() {
                self.cache_counting_history();
            }
            return;
        }
        if self.dirty || self.startup_error.is_some() {
            return;
        }
        match command {
            Command::Deal => {
                let at = match self.now() {
                    Ok(at) => at,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                match self.table.deal() {
                    Ok(()) => {
                        self.just_unlocked = false;
                        self.table_feedback.clear();
                        self.error = None;
                        self.finish_round(false, at);
                        self.refresh_analytics();
                        // A natural can finish a round without a decision.
                        if self.table.result.is_some() {
                            self.persist_progress();
                        }
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
            Command::Act(action) => {
                let at = match self.now() {
                    Ok(at) => at,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                let practice = self.page == Page::Practice;
                let game = if practice {
                    self.practice.as_mut()
                } else {
                    Some(&mut self.table)
                };
                let Some(game) = game else {
                    return;
                };
                let Some(situation) = game.situation() else {
                    if game.phase == Phase::Playing {
                        self.error = Some(
                            GameError::InvalidState("the dealer upcard is missing").to_string(),
                        );
                    }
                    return;
                };
                if !situation.allows(action) {
                    return;
                }
                let recommendation = strategy::recommendation(situation);
                if let Err(error) = game.act(action) {
                    self.error = Some(error.to_string());
                    return;
                }
                let mode = if practice {
                    StudyMode::Practice
                } else {
                    StudyMode::Table
                };
                self.profile.record_attempt(situation, action, at, mode);
                let feedback = Feedback {
                    situation,
                    chosen: action,
                    expected: recommendation.action,
                    explanation: recommendation.explanation,
                };
                if practice {
                    self.practice_feedback.push(feedback);
                } else {
                    self.table_feedback.push(feedback);
                }
                self.finish_round(practice, at);
                self.refresh_analytics();
                self.persist_progress();
            }
            Command::StartPractice(queue) => {
                if !self.profile.assessment_unlocked() || queue.is_empty() {
                    return;
                }
                self.practice_total = queue.len();
                self.practice_completed = 0;
                self.practice_start_attempt = self.profile.attempts.len();
                self.practice_queue = queue.into();
                self.page = Page::Practice;
                self.start_next_practice();
            }
            Command::NextPractice => self.start_next_practice(),
            Command::StartCounting => self.reset_counting_trial(),
            Command::NextCounting => {
                if let Some(trial) = self.counting.as_mut()
                    && !trial.is_complete()
                    && let Err(error) = trial.next_card()
                {
                    self.error = Some(format!("Cannot reveal the next card: {error}"));
                }
            }
            Command::FinishCounting => {
                if self
                    .counting
                    .as_ref()
                    .is_some_and(CountingTrial::is_complete)
                {
                    self.counting_elapsed = self
                        .counting_started_at
                        .take()
                        .map(|started| started.elapsed());
                    self.counting_input_error = None;
                }
            }
            Command::SubmitCounting => {
                if self.counting_assessment.is_some() {
                    return;
                }
                let Some(submitted) = parse_submitted_count(&self.counting_input) else {
                    self.counting_input_error =
                        Some("Enter a signed whole number, such as +3 or -2.".to_owned());
                    return;
                };
                let Some(trial) = self.counting.as_ref() else {
                    return;
                };
                let Some(elapsed) = self.counting_elapsed else {
                    return;
                };
                let actual = trial.actual_count();
                let correct = submitted == actual;
                let at = if correct {
                    Some(match self.now() {
                        Ok(at) => at,
                        Err(error) => {
                            self.error = Some(error);
                            return;
                        }
                    })
                } else {
                    None
                };
                self.counting_input_error = None;
                self.counting_assessment = Some(CountingAssessment { submitted, actual });
                if let Some(at) = at {
                    self.profile
                        .record_counting_trial(at, elapsed.as_millis() as u64);
                    if self.persist_progress() {
                        self.cache_counting_history();
                    }
                }
            }
            Command::SuiteSetCountEvery(interval) => {
                if interval.is_none_or(|value| matches!(value, 1 | 3 | 5 | 10 | 25)) {
                    self.profile.suite_settings.count_every = interval;
                    if let Some(SuiteActivity::RunningCount(drill)) = self.suite.as_mut() {
                        drill.interval = interval;
                    }
                    self.persist_progress();
                }
            }
            Command::SuiteSetDealingSpeed(speed) => {
                if let Some(delay) = dealing_delay_for_speed(speed) {
                    self.profile.suite_settings.dealing_delay_ms = delay;
                    self.persist_progress();
                }
            }
            Command::SuiteTrueCountAngle(direction) => {
                if let Some(SuiteActivity::TrueCount(drill)) = self.suite.as_mut() {
                    drill.turn_angle(direction);
                }
            }
            Command::SuiteAdjustInput(delta) => {
                let value = self.suite_input.parse::<i32>().unwrap_or_default();
                self.suite_input = value.saturating_add(delta).to_string();
            }
            Command::SuiteSetComparison(comparison) => {
                self.suite_comparison = comparison;
            }
            Command::SuiteSetTrueCountDecks(decks) => {
                if matches!(decks, 2 | 4 | 6 | 8) {
                    self.profile.suite_settings.true_count_decks = decks;
                    self.persist_progress();
                }
            }
            Command::SuiteSetHalfDeck(half_deck) => {
                self.profile.suite_settings.half_deck = half_deck;
                self.persist_progress();
            }
            Command::SuiteSetTrueCountDuration(seconds) => {
                if matches!(seconds, 90 | 150 | 300 | 600) {
                    self.profile.suite_settings.true_count_duration_secs = seconds;
                    self.persist_progress();
                }
            }
            Command::ResetSuiteCurrent => {
                self.profile.reset_suite_current_stats();
                self.persist_progress();
            }
            Command::StartSuite(tool) => {
                self.start_suite(tool);
                if let Some(SuiteActivity::PressureTable(table)) = self.suite.as_mut() {
                    if let Err(error) = table.deal() {
                        self.error = Some(format!("Cannot start pressure round: {error}"));
                    }
                    self.begin_pressure_prompt();
                }
            }
            Command::SuiteAct(action) => {
                if self.suite_pressure_prompt.is_some()
                    || (self.suite_feedback.is_some()
                        && matches!(
                            self.suite,
                            Some(SuiteActivity::BasicStrategy(_) | SuiteActivity::CoreIndex(_))
                        ))
                {
                    return;
                }
                let mut pressure_action_taken = false;
                let outcome = match self.suite.as_mut() {
                    Some(SuiteActivity::BasicStrategy(drill)) => Ok(drill.submit(action)),
                    Some(SuiteActivity::CoreIndex(drill)) => {
                        if drill
                            .current()
                            .is_some_and(|scenario| scenario.situation.allows(action))
                        {
                            self.suite_selected_action = Some(action);
                        }
                        Ok(None)
                    }
                    Some(SuiteActivity::TablePractice(table)) => {
                        table.act_and_grade(action).map(Some)
                    }
                    Some(SuiteActivity::PressureTable(table)) => {
                        table.act_and_grade(action).map(|feedback| {
                            pressure_action_taken = true;
                            Some(feedback)
                        })
                    }
                    Some(SuiteActivity::Assessment(session)) => {
                        session.act_and_grade(action).map(Some)
                    }
                    _ => Ok(None),
                };
                match outcome {
                    Ok(Some(feedback)) => {
                        let bucket = if matches!(
                            feedback.expected,
                            Action::Insure | Action::DeclineInsurance
                        ) {
                            GradeBucket::Insurance
                        } else if feedback.source == strategy::StrategySource::CoreIndex {
                            GradeBucket::Deviation
                        } else {
                            GradeBucket::BasicStrategy
                        };
                        self.suite_feedback = Some(feedback);
                        self.suite_record(bucket, feedback.correct);
                    }
                    Ok(None) => {}
                    Err(error) => self.error = Some(format!("Cannot play suite action: {error}")),
                }
                if pressure_action_taken {
                    if let Some(started) = self.suite_pressure_decision_started_at.take() {
                        let elapsed = started.elapsed();
                        let timely = elapsed
                            <= Duration::from_secs(u64::from(PressureTable::DECISION_LIMIT_SECS));
                        self.suite_record(GradeBucket::Timing, timely);
                        if !timely {
                            self.suite_message = Some(format!(
                                "Late decision: {:.1}s · limit {}s",
                                elapsed.as_secs_f32(),
                                PressureTable::DECISION_LIMIT_SECS
                            ));
                        }
                    }
                    self.begin_pressure_prompt();
                }
            }
            Command::SuiteNext => {
                let completed_tool = match self.suite.as_ref() {
                    Some(SuiteActivity::BasicStrategy(drill)) if drill.is_finished() => {
                        Some(ToolId::BasicStrategy)
                    }
                    Some(SuiteActivity::CoreIndex(drill)) if drill.is_finished() => {
                        Some(ToolId::CoreIndexPlays)
                    }
                    _ => None,
                };
                let result = match self.suite.as_mut() {
                    Some(SuiteActivity::TrueCount(drill)) if drill.feedback.is_some() => {
                        let seed = self.suite_seed;
                        self.suite_seed = self.suite_seed.wrapping_add(1);
                        let settings = &self.profile.suite_settings;
                        match TrueCountDrill::new(
                            seed,
                            settings.true_count_decks,
                            settings.half_deck,
                            settings.true_count_duration_secs,
                        ) {
                            Ok(drill) => self.suite = Some(SuiteActivity::TrueCount(drill)),
                            Err(error) => {
                                self.error =
                                    Some(format!("Cannot start True Count round: {error}"));
                                return;
                            }
                        }
                        Ok(())
                    }
                    Some(SuiteActivity::BasicStrategy(_) | SuiteActivity::CoreIndex(_))
                        if self.suite_feedback.is_some() =>
                    {
                        Ok(())
                    }
                    Some(SuiteActivity::TablePractice(table))
                        if matches!(table.game.phase, Phase::Ready | Phase::Finished)
                            && !table.pending_count =>
                    {
                        table.deal()
                    }
                    Some(SuiteActivity::PressureTable(table))
                        if matches!(table.table.game.phase, Phase::Ready | Phase::Finished)
                            && !table.table.pending_count
                            && self.suite_pressure_prompt.is_none() =>
                    {
                        table.deal()
                    }
                    Some(SuiteActivity::Assessment(session))
                        if matches!(session.table.game.phase, Phase::Ready | Phase::Finished)
                            && !session.table.pending_count
                            && !session.table.past_cut() =>
                    {
                        session.deal()
                    }
                    _ => return,
                };
                if let Err(error) = result {
                    self.error = Some(format!("Cannot deal suite round: {error}"));
                    return;
                }
                self.suite_feedback = None;
                self.suite_message = None;
                self.suite_input = "0".to_owned();
                self.suite_selected_action = None;
                self.suite_comparison = IndexComparison::GreaterOrEqual;
                self.begin_pressure_prompt();
                if let Some(tool) = completed_tool {
                    self.suite_finish(tool, true, Some(self.suite_correct == self.suite_attempts));
                    self.suite = None;
                    self.suite_message = Some(format!("{} drill complete.", tool.label()));
                }
            }
            Command::SuiteSubmit => {
                let input = parse_submitted_count(&self.suite_input);
                let selected_action = self.suite_selected_action;
                let comparison = self.suite_comparison;
                let mut running_count_feedback = None;
                let mut record = None;
                if let Some(activity) = self.suite.as_mut() {
                    match activity {
                        SuiteActivity::RunningCount(drill) => {
                            if let Some(submitted) = input
                                && let Some(feedback) = drill.submit_count(submitted)
                            {
                                running_count_feedback = Some(feedback);
                                record = Some((GradeBucket::Count, feedback.correct));
                            }
                        }
                        SuiteActivity::TrueCount(drill) => {
                            if let Some(submitted) = input
                                && drill.feedback.is_none()
                            {
                                let feedback = drill.submit(submitted);
                                record = Some((GradeBucket::Count, feedback.correct));
                                self.suite_message = Some(if feedback.within_quarter_deck {
                                    "Correct · within a quarter deck".to_owned()
                                } else {
                                    format!(
                                        "{} · expected {}",
                                        if feedback.correct {
                                            "Correct"
                                        } else {
                                            "Incorrect"
                                        },
                                        feedback.expected
                                    )
                                });
                            }
                        }
                        SuiteActivity::TablePractice(table) => {
                            if let Some(submitted) = input
                                && table.pending_count
                            {
                                let correct = table.submit_count(submitted);
                                record = Some((GradeBucket::Count, correct));
                                self.suite_message = Some(format!(
                                    "{} · expected {:+}",
                                    if correct { "Correct" } else { "Incorrect" },
                                    table.expected_count()
                                ));
                            }
                        }
                        SuiteActivity::PressureTable(table) => {
                            if let Some(submitted) = input {
                                if self.suite_pressure_prompt.is_some() {
                                    record = table.submit_pressure(submitted);
                                    if let Some((_, correct)) = record {
                                        self.suite_message = Some(if correct {
                                            "Correct pressure response.".to_owned()
                                        } else {
                                            "Incorrect pressure response.".to_owned()
                                        });
                                    }
                                } else if table.pending_count() {
                                    let correct = table.submit_count(submitted);
                                    record = Some((GradeBucket::Count, correct));
                                    self.suite_message = Some(format!(
                                        "{} · expected {:+}",
                                        if correct { "Correct" } else { "Incorrect" },
                                        table.expected_count()
                                    ));
                                }
                            }
                        }
                        SuiteActivity::CoreIndex(drill) => {
                            if self.suite_feedback.is_none()
                                && let (Some(action), Some(index), Some(scenario)) =
                                    (selected_action, input, drill.current())
                                && let Some(feedback) =
                                    drill.submit(action, comparison, index as f32)
                            {
                                record = Some((GradeBucket::Deviation, feedback.correct));
                                self.suite_feedback = Some(feedback);
                                self.suite_message = Some(format!(
                                    "{} · your {} at TC {} {} · correct {} at TC {} {}",
                                    if feedback.correct {
                                        "Correct"
                                    } else {
                                        "Incorrect"
                                    },
                                    action.label(),
                                    comparison_label(comparison),
                                    index,
                                    scenario.play.action.label(),
                                    comparison_label(scenario.play.comparison),
                                    scenario.play.threshold
                                ));
                            }
                        }
                        SuiteActivity::Assessment(session) => {
                            if let Some(submitted) = input
                                && let Some(correct) = session.submit_count(submitted)
                            {
                                self.suite_message = Some(format!(
                                    "{} · expected {:+}",
                                    if correct { "Correct" } else { "Incorrect" },
                                    session.table.expected_count()
                                ));
                                record = Some((GradeBucket::Count, correct));
                            }
                        }
                        SuiteActivity::BasicStrategy(_) => {}
                    }
                }
                if let Some((bucket, correct)) = record {
                    self.suite_record(bucket, correct);
                }
                if let Some(feedback) = running_count_feedback {
                    if feedback.final_count {
                        let passed = self.suite_attempts == self.suite_correct;
                        self.suite_finish(ToolId::RunningCount, true, Some(passed));
                        self.suite = None;
                        self.suite_running_next_step_at = None;
                        self.suite_message = Some(if passed {
                            "Running Count report: PASSED.".to_owned()
                        } else {
                            format!("Final count: actual {:+}.", feedback.expected)
                        });
                    } else {
                        self.suite_running_next_step_at = None;
                        if self.suite_running_clear_at.is_none() {
                            self.suite_running_clear_at =
                                Some(Instant::now() + RUNNING_COUNT_ROUND_CLEAR_PAUSE);
                        }
                        self.suite_message = (!feedback.correct).then(|| {
                            format!("Wrong · actual running count: {:+}", feedback.expected)
                        });
                    }
                }
                if record.is_some() {
                    self.suite_input = "0".to_owned();
                    self.begin_pressure_prompt();
                } else if input.is_none() {
                    self.suite_message = Some("Enter a signed whole number.".to_owned());
                }
            }
            Command::SuiteRestart => {
                if matches!(self.suite, Some(SuiteActivity::RunningCount(_))) {
                    self.suite_finish(ToolId::RunningCount, false, None);
                    self.start_suite(ToolId::RunningCount);
                }
            }
            Command::SuiteEnd => {
                let tool = match self.suite {
                    Some(SuiteActivity::RunningCount(_)) => ToolId::RunningCount,
                    Some(SuiteActivity::TrueCount(_)) => ToolId::TrueCount,
                    Some(SuiteActivity::BasicStrategy(_)) => ToolId::BasicStrategy,
                    Some(SuiteActivity::CoreIndex(_)) => ToolId::CoreIndexPlays,
                    Some(SuiteActivity::TablePractice(_)) => ToolId::TablePractice,
                    Some(SuiteActivity::Assessment(_)) => ToolId::Assessment,
                    Some(SuiteActivity::PressureTable(_)) => ToolId::PressureTable,
                    None => return,
                };
                if let Some(SuiteActivity::Assessment(session)) = self.suite.as_mut() {
                    session.exit_early();
                }
                self.suite_finish(tool, false, None);
                self.suite = None;
                self.suite_pressure_decision_started_at = None;
                self.suite_running_next_step_at = None;
                self.suite_message = Some("Session ended early. No pass is recorded.".to_owned());
            }
            Command::SuiteCompleteShoe => {
                let mut report = None;
                if let Some(SuiteActivity::Assessment(session)) = self.suite.as_mut() {
                    if session.finish_shoe() {
                        if session.shoes_completed == session.shoe_limit {
                            report = Some(session.report());
                        } else {
                            self.suite_message =
                                Some(format!("Shoe {} of 3 complete.", session.shoes_completed));
                        }
                    } else {
                        self.suite_message =
                            Some("Finish the current shoe through the cut card first.".to_owned());
                    }
                }
                if let Some(report) = report {
                    self.suite_finish(ToolId::Assessment, true, Some(report.passed));
                    self.suite = None;
                    self.suite_message = Some(if report.passed {
                        "Assessment report: PASSED · exact perfect session.".to_owned()
                    } else {
                        "Assessment report: NOT PASSED · every answer must be correct.".to_owned()
                    });
                }
            }
            Command::RetrySave => {}
        }
    }

    fn shortcut(&self, ctx: &egui::Context) -> Option<Command> {
        if self.dirty {
            return None;
        }
        let wants_keyboard_input = ctx.egui_wants_keyboard_input();
        ctx.input(|input| {
            if input.modifiers.any() {
                return None;
            }
            if wants_keyboard_input {
                return (self.page == Page::TrainingSuite
                    && self.suite.is_some()
                    && input.key_pressed(egui::Key::Enter))
                .then_some(Command::SuiteSubmit);
            }
            if self.page == Page::TrainingSuite {
                if let Some(SuiteActivity::RunningCount(drill)) = self.suite.as_ref() {
                    if input.key_pressed(egui::Key::M) {
                        return Some(Command::SuiteEnd);
                    }
                    if drill.checkpoint.prompt {
                        if input.key_pressed(egui::Key::ArrowLeft) {
                            return Some(Command::SuiteAdjustInput(-1));
                        }
                        if input.key_pressed(egui::Key::ArrowRight) {
                            return Some(Command::SuiteAdjustInput(1));
                        }
                    }
                }
                if input.key_pressed(egui::Key::Enter) {
                    return Some(Command::SuiteSubmit);
                }
                let situation = match self.suite.as_ref()? {
                    SuiteActivity::BasicStrategy(drill) => drill.current()?.situation,
                    SuiteActivity::CoreIndex(drill) => drill.current()?.situation,
                    SuiteActivity::TablePractice(table) => table.game.situation()?,
                    SuiteActivity::PressureTable(table) => table.table.game.situation()?,
                    SuiteActivity::Assessment(session) => session.table.game.situation()?,
                    SuiteActivity::RunningCount(_) | SuiteActivity::TrueCount(_) => return None,
                };
                return [
                    (egui::Key::H, Action::Hit),
                    (egui::Key::S, Action::Stand),
                    (egui::Key::D, Action::Double),
                    (egui::Key::P, Action::Split),
                    (egui::Key::R, Action::Surrender),
                    (egui::Key::I, Action::Insure),
                    (egui::Key::N, Action::DeclineInsurance),
                ]
                .into_iter()
                .find(|(key, action)| input.key_pressed(*key) && situation.allows(*action))
                .map(|(_, action)| Command::SuiteAct(action));
            }
            if self.page == Page::Counting {
                let trial = self.counting.as_ref()?;
                if input.key_pressed(egui::Key::Space)
                    && self.counting_elapsed.is_none()
                    && self.counting_assessment.is_none()
                {
                    return Some(if trial.is_complete() {
                        Command::FinishCounting
                    } else {
                        Command::NextCounting
                    });
                }
                return None;
            }
            let game = match self.page {
                Page::Table => &self.table,
                Page::Practice => self.practice.as_ref()?,
                _ => return None,
            };
            if input.key_pressed(egui::Key::Enter) {
                return match (self.page, game.phase) {
                    (Page::Table, Phase::Ready | Phase::Finished) => Some(Command::Deal),
                    (Page::Practice, Phase::Finished) => Some(Command::NextPractice),
                    _ => None,
                };
            }
            let situation = game.situation()?;
            [
                (egui::Key::H, Action::Hit),
                (egui::Key::S, Action::Stand),
                (egui::Key::D, Action::Double),
                (egui::Key::P, Action::Split),
                (egui::Key::R, Action::Surrender),
                (egui::Key::I, Action::Insure),
                (egui::Key::N, Action::DeclineInsurance),
            ]
            .into_iter()
            .find(|(key, action)| input.key_pressed(*key) && situation.allows(*action))
            .map(|(_, action)| Command::Act(action))
        })
    }
}

impl eframe::App for TrainerApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = root.ctx().clone();
        self.tick_running_count(Instant::now());
        let running_count_animation_active =
            matches!(self.suite.as_ref(), Some(SuiteActivity::RunningCount(_)))
                && (self.suite_running_next_step_at.is_some()
                    || self.suite_running_clear_at.is_some());
        let repaint_after = if self.page == Page::Counting && self.counting_started_at.is_some() {
            Duration::from_millis(100)
        } else if running_count_animation_active {
            Duration::from_millis(16)
        } else if self.page == Page::TrainingSuite && self.suite_started_at.is_some() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(30)
        };
        ctx.request_repaint_after(repaint_after);

        let true_count_timed_out = matches!(
            self.suite.as_ref(),
            Some(SuiteActivity::TrueCount(drill))
                if self.suite_started_at.is_some_and(|started| {
                    started.elapsed().as_secs() >= drill.duration_secs
                })
        );
        if true_count_timed_out {
            let passed = self.suite_attempts > 0 && self.suite_correct == self.suite_attempts;
            self.suite_finish(ToolId::TrueCount, true, Some(passed));
            self.suite = None;
        }
        if self.dirty && !self.allow_close && ctx.input(|input| input.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close_warning = true;
        }
        if self.startup_error.is_some() {
            self.startup_error_view(root);
            return;
        }
        let mut command = self.shortcut(&ctx);
        self.sidebar(root);
        let feedback: &[Feedback] = match self.page {
            Page::Table => &self.table_feedback,
            Page::Insights if self.just_unlocked => &self.table_feedback,
            Page::Practice if self.practice.is_some() => &self.practice_feedback,
            _ => &[],
        };
        // Feedback stays visible even when a split round makes the table scroll.
        if !feedback.is_empty() {
            egui::Panel::bottom("decision-feedback")
                .resizable(false)
                .frame(
                    egui::Frame::NONE
                        .fill(widgets::BG)
                        .inner_margin(egui::Margin {
                            left: 28,
                            right: 28,
                            top: 0,
                            bottom: 14,
                        }),
                )
                .show(root, |ui| widgets::feedback(ui, feedback));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(widgets::BG).inner_margin(28)).show(root, |ui| {
                if let Some(error) = &self.error {
                    let message = error.clone();
                    widgets::panel().stroke(egui::Stroke::new(1.0, widgets::GOLD)).show(ui, |ui| {
                        ui.colored_label(widgets::GOLD, message);
                        if self.dirty {
                            ui.label("Play is paused to protect your progress. Retry saving before continuing.");
                            if ui.button("Retry save").clicked() { command = Some(Command::RetrySave); }
                        } else if ui.small_button("Dismiss").clicked() { self.error = None; }
                        if self.close_warning {
                            ui.label("Closing now will discard the unsaved decisions.");
                            if ui.button("Close without saving").clicked() {
                                self.allow_close = true;
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                    });
                    ui.add_space(14.0);
                }
            let scroll_id = egui::Id::new("page-scroll").with(self.page as u8);
            let scroll_id = if self.page == Page::TrainingSuite {
                scroll_id.with(self.suite.as_ref().map(std::mem::discriminant))
            } else {
                scroll_id
            };
            egui::ScrollArea::vertical().id_salt(scroll_id).show(ui, |ui| {
                ui.add_enabled_ui(!self.dirty, |ui| {
                    match self.page {
                        Page::Table => self.table_view(ui, &mut command),
                        Page::Insights => self.insights_view(ui, &mut command),
                        Page::Practice => self.practice_view(ui, &mut command),
                        Page::Counting => self.counting_view(ui, &mut command),
                        Page::TrainingSuite => self.training_suite_view(ui, &mut command),
                        Page::Rules => self.rules_view(ui),
                    }
                });
            });
        });
        if let Some(command) = command {
            self.execute(command);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::App;
    use tempfile::{TempDir, tempdir};
    use twenty_one_pro::{counting::TRIAL_SIZE, training::SCHEMA_VERSION};

    fn test_app(store: Store) -> TrainerApp {
        let mut app = TrainerApp::with_profile(
            &egui::Context::default(),
            Profile::default(),
            Some(store),
            PathBuf::new(),
            Game::new(1),
        );
        app.page = Page::Counting;
        app
    }

    fn app_with_store() -> (TempDir, TrainerApp) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("profile.redb");
        let store = Store::open(&path).unwrap();
        let app = TrainerApp::with_profile(
            &egui::Context::default(),
            Profile::default(),
            Some(store),
            path,
            Game::new(0),
        );
        (directory, app)
    }

    fn finish_table_with_strategy(app: &mut TrainerApp) {
        while app.table.phase != Phase::Finished {
            let situation = app.table.situation().unwrap();
            app.execute(Command::Act(strategy::recommendation(situation).action));
        }
    }

    #[test]
    fn empty_data_directory_override_is_ignored() {
        assert_eq!(configured_data_path(Some(OsString::new())), None);
        assert_eq!(
            configured_data_path(Some(OsString::from("/tmp/21pro"))),
            Some(PathBuf::from("/tmp/21pro/profile.redb"))
        );
    }

    #[test]
    fn timestamp_rejects_clock_before_unix_epoch() {
        let before_epoch = UNIX_EPOCH - Duration::from_secs(7);
        assert!(TrainerApp::unix_timestamp(before_epoch).is_err());
    }

    #[test]
    fn malformed_table_action_surfaces_state_error() {
        let mut app = TrainerApp::with_profile(
            &egui::Context::default(),
            Profile::default(),
            None,
            PathBuf::new(),
            Game::new(1),
        );
        app.table.deal().unwrap();
        app.table.dealer.clear();

        app.execute(Command::Act(Action::Hit));

        assert_eq!(
            app.error.as_deref(),
            Some("The game state is missing cards required for this action.")
        );
    }

    #[test]
    fn natural_deal_records_and_persists_one_original_round() {
        let (_directory, mut app) = app_with_store();
        for seed in 0..1000 {
            app.table = Game::new(seed);
            app.execute(Command::Deal);
            if app.table.result.is_some() {
                break;
            }
        }
        assert!(app.table.result.is_some());
        assert_eq!(app.profile.rounds_played(), 1);
        assert_eq!(
            app.store.as_ref().unwrap().load().unwrap().rounds_played(),
            1
        );
    }

    fn finish_counting_trial(app: &mut TrainerApp, seed: u64) -> i32 {
        app.reset_counting_trial_with_seed(seed);
        for _ in 1..TRIAL_SIZE {
            app.execute(Command::NextCounting);
        }
        assert_eq!(app.counting.as_ref().unwrap().cards_seen(), TRIAL_SIZE);
        app.execute(Command::FinishCounting);
        assert!(app.counting_started_at.is_none());
        assert!(app.counting_elapsed.is_some());
        app.counting.as_ref().unwrap().actual_count()
    }

    #[test]
    fn counting_controller_assesses_once_and_saves_only_correct_trials() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("profile.redb");
        let store = Store::open(&path).unwrap();
        let mut app = test_app(store);

        let actual = finish_counting_trial(&mut app, 42);
        let elapsed = app.counting_elapsed.unwrap();
        assert_eq!(app.counting_duration(), elapsed);

        app.counting_input = "not a count".to_owned();
        app.execute(Command::SubmitCounting);
        assert!(app.counting_assessment.is_none());
        assert!(app.counting_input_error.is_some());
        assert!(app.profile.counting_history.is_empty());

        app.counting_input = (actual + 1).to_string();
        app.execute(Command::SubmitCounting);
        assert!(
            !app.counting_assessment.as_ref().unwrap().correct(),
            "the deliberately wrong answer must be assessed as incorrect"
        );
        assert!(app.profile.counting_history.is_empty());

        app.counting_input = actual.to_string();
        app.execute(Command::SubmitCounting);
        assert!(app.profile.counting_history.is_empty());

        let actual = finish_counting_trial(&mut app, 7);
        let elapsed = app.counting_elapsed.unwrap();
        app.counting_input = actual.to_string();
        app.execute(Command::SubmitCounting);
        assert!(app.counting_assessment.as_ref().unwrap().correct());
        assert_eq!(app.profile.counting_history.len(), 1);
        assert_eq!(
            app.profile.counting_history[0].duration_ms,
            elapsed.as_millis() as u64
        );

        app.execute(Command::SubmitCounting);
        assert_eq!(app.profile.counting_history.len(), 1);

        drop(app);
        let reopened = Store::open(&path).unwrap();
        assert_eq!(reopened.load().unwrap().counting_history.len(), 1);
    }
    #[test]
    fn failed_counting_save_stays_pending_until_retry() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("profile.redb");
        let store = Store::open(&path).unwrap();
        let mut app = test_app(store);
        let actual = finish_counting_trial(&mut app, 42);
        app.profile.schema_version += 1;
        app.counting_input = actual.to_string();
        app.execute(Command::SubmitCounting);

        assert!(app.dirty);
        assert!(app.counting_assessment.as_ref().unwrap().correct());
        assert_eq!(app.profile.counting_history.len(), 1);
        assert!(app.counting_history.is_empty());

        app.profile.schema_version -= 1;
        app.execute(Command::RetrySave);
        assert!(!app.dirty);
        assert_eq!(app.counting_history.len(), 1);
        drop(app);
        assert_eq!(
            Store::open(&path)
                .unwrap()
                .load()
                .unwrap()
                .counting_history
                .len(),
            1
        );
    }

    fn spacebar_context() -> egui::Context {
        let context = egui::Context::default();
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |_| {},
        );
        output.textures_delta.clear();
        context
    }

    #[test]
    fn spacebar_dispatches_next_and_finish_for_counting_trial() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("profile.redb");
        let store = Store::open(&path).unwrap();
        let mut app = test_app(store);
        app.reset_counting_trial_with_seed(42);

        let context = spacebar_context();
        assert!(matches!(
            app.shortcut(&context),
            Some(Command::NextCounting)
        ));
        app.execute(Command::NextCounting);
        for _ in 2..TRIAL_SIZE {
            app.execute(Command::NextCounting);
        }
        assert_eq!(app.counting.as_ref().unwrap().cards_seen(), TRIAL_SIZE);

        let context = spacebar_context();
        assert!(matches!(
            app.shortcut(&context),
            Some(Command::FinishCounting)
        ));
        app.execute(Command::FinishCounting);
        assert!(app.counting_elapsed.is_some());
    }

    #[test]
    fn split_settlement_records_and_persists_one_original_round() {
        let (_directory, mut app) = app_with_store();
        let target = Situation {
            kind: HandKind::Pair,
            value: 8,
            dealer: 6,
            can_double: true,
            can_split: true,
            can_surrender: true,
            split_aces: false,
        };
        app.table = Game::practice(target, 0).unwrap();
        app.execute(Command::Act(Action::Split));
        finish_table_with_strategy(&mut app);

        assert_eq!(app.profile.rounds_played(), 1);
        assert!(app.profile.attempts.len() >= 2);
        assert_eq!(
            app.store.as_ref().unwrap().load().unwrap().rounds_played(),
            1
        );
    }

    #[test]
    fn failed_save_pauses_play_and_close_until_retry_or_discard() {
        let (_directory, mut app) = app_with_store();
        let target = Situation {
            kind: HandKind::Hard,
            value: 16,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: true,
            split_aces: false,
        };
        app.table = Game::practice(target, 0).unwrap();
        app.profile.schema_version = SCHEMA_VERSION + 1;
        app.execute(Command::Act(Action::Stand));

        assert!(app.dirty);
        assert!(app.error.is_some());
        let attempts = app.profile.attempts.len();
        app.execute(Command::Act(Action::Hit));
        assert_eq!(app.profile.attempts.len(), attempts);

        app.page = Page::Rules;
        let context = egui::Context::default();
        let mut viewports = egui::ViewportIdMap::default();
        viewports.insert(
            egui::ViewportId::ROOT,
            egui::ViewportInfo {
                events: vec![egui::ViewportEvent::Close],
                ..Default::default()
            },
        );
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = context.run_ui(
            egui::RawInput {
                viewport_id: egui::ViewportId::ROOT,
                viewports,
                ..Default::default()
            },
            |ui| app.ui(ui, &mut frame),
        );
        output.textures_delta.clear();
        let root = output.viewport_output.get(&egui::ViewportId::ROOT).unwrap();
        assert!(root.commands.contains(&egui::ViewportCommand::CancelClose));
        assert!(app.close_warning);
        assert!(!app.allow_close);

        app.profile.schema_version = SCHEMA_VERSION;
        app.execute(Command::RetrySave);
        assert!(!app.dirty);
        assert!(app.error.is_none());
        assert_eq!(
            app.store.as_ref().unwrap().load().unwrap().rounds_played(),
            1
        );
    }

    #[test]
    fn true_count_is_graded_once_and_a_perfect_timed_session_passes() {
        let (_directory, mut app) = app_with_store();
        app.execute(Command::StartSuite(ToolId::TrueCount));
        let answer = match app.suite.as_ref().unwrap() {
            SuiteActivity::TrueCount(drill) => drill.answer(),
            _ => unreachable!(),
        };
        app.suite_input = answer.to_string();
        app.execute(Command::SuiteSubmit);
        app.suite_input = answer.to_string();
        app.execute(Command::SuiteSubmit);
        assert_eq!((app.suite_attempts, app.suite_correct), (1, 1));
        app.suite_started_at = Some(Instant::now() - Duration::from_secs(91));
        let context = egui::Context::default();
        let mut frame = eframe::Frame::_new_kittest();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| app.ui(ui, &mut frame));
        output.textures_delta.clear();
        assert!(app.suite.is_none());
        let report = app.suite_report.as_ref().unwrap();
        assert_eq!(report.passed, Some(true));
        assert_eq!((report.attempts, report.correct), (1, 1));
        let saved = app.store.as_ref().unwrap().load().unwrap();
        assert_eq!(saved.suite_sessions.last().unwrap().passed, Some(true));
        assert_eq!(saved.suite_stats.attempts, 1);
        assert!(saved.rounds.is_empty());
        assert!(saved.attempts.is_empty());
    }

    #[test]
    fn suite_counts_cannot_be_graded_without_an_unanswered_checkpoint() {
        let (_directory, mut app) = app_with_store();
        app.suite_seed = 7;
        app.execute(Command::StartSuite(ToolId::TablePractice));
        app.execute(Command::SuiteSubmit);
        assert_eq!(app.suite_attempts, 0);
        app.execute(Command::SuiteNext);
        app.execute(Command::SuiteAct(Action::Stand));
        let answer = match app.suite.as_ref().unwrap() {
            SuiteActivity::TablePractice(table) => {
                assert!(table.pending_count);
                table.expected_count()
            }
            _ => unreachable!(),
        };
        app.suite_input = answer.to_string();
        app.execute(Command::SuiteSubmit);
        app.suite_input = answer.to_string();
        app.execute(Command::SuiteSubmit);
        assert_eq!(
            app.profile.suite_stats.buckets[&GradeBucket::Count].attempts,
            1
        );
    }
}
