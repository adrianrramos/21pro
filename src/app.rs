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
    collections::VecDeque,
    ffi::OsString,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use twenty_one_pro::{
    counting::{CountingRecord, CountingTrial, parse_submitted_count, sorted_history},
    game::{Game, Phase},
    model::{Action, HandKind, Situation},
    storage::{self, Store},
    strategy,
    training::{Analytics, CellStats, Profile, StudyMode},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Table,
    Insights,
    Practice,
    Counting,
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

enum Command {
    Deal,
    Act(Action),
    StartPractice(Vec<Situation>),
    NextPractice,
    StartCounting,
    NextCounting,
    FinishCounting,
    SubmitCounting,
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

    fn now(&self) -> i64 {
        #[cfg(any(test, feature = "dev-fixtures"))]
        if self.fixture.is_some() {
            return fixtures::NOW;
        }
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after 1970")
            .as_secs() as i64
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
    fn finish_round(&mut self, practice: bool) {
        if practice {
            if let Some(game) = &self.practice
                && game.phase == Phase::Finished
            {
                self.practice_completed += 1;
            }
        } else if let Some(result) = &self.table.result {
            let was_unlocked = self.profile.assessment_unlocked();
            self.profile.record_round(self.now(), result.net_half_units);
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
                match self.table.deal() {
                    Ok(()) => {
                        self.just_unlocked = false;
                        self.table_feedback.clear();
                        self.error = None;
                        self.finish_round(false);
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
                self.profile
                    .record_attempt(situation, action, self.now(), mode);
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
                self.finish_round(practice);
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
                self.counting_input_error = None;
                self.counting_assessment = Some(CountingAssessment { submitted, actual });
                if correct {
                    self.profile
                        .record_counting_trial(self.now(), elapsed.as_millis() as u64);
                    if self.persist_progress() {
                        self.cache_counting_history();
                    }
                }
            }
            Command::RetrySave => unreachable!(),
        }
    }

    fn shortcut(&self, ctx: &egui::Context) -> Option<Command> {
        if self.dirty || ctx.egui_wants_keyboard_input() {
            return None;
        }
        ctx.input(|input| {
            if input.modifiers.any() {
                return None;
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
        ctx.request_repaint_after(
            if self.page == Page::Counting && self.counting_started_at.is_some() {
                Duration::from_millis(100)
            } else {
                Duration::from_secs(30)
            },
        );
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
            egui::ScrollArea::vertical().id_salt(egui::Id::new("page-scroll").with(self.page as u8)).show(ui, |ui| {
                ui.add_enabled_ui(!self.dirty, |ui| {
                    match self.page {
                        Page::Table => self.table_view(ui, &mut command),
                        Page::Insights => self.insights_view(ui, &mut command),
                        Page::Practice => self.practice_view(ui, &mut command),
                        Page::Counting => self.counting_view(ui, &mut command),
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
}
