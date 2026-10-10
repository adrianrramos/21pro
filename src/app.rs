//! Desktop orchestration: render from state, then apply at most one user command.
//! The game engine never depends on egui, and progress is saved after each decision.
#[cfg(any(test, feature = "dev-fixtures"))]
mod fixtures;
mod views;
mod widgets;

use eframe::egui;
use std::{
    collections::VecDeque,
    ffi::OsString,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use twenty_one_pro::{
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

enum Command {
    Deal,
    Act(Action),
    StartPractice(Vec<Situation>),
    NextPractice,
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

    fn persist_progress(&mut self) {
        self.dirty = true;
        let Some(store) = &self.store else {
            return;
        };
        match store.save(&self.profile) {
            Ok(()) => {
                self.dirty = false;
                self.error = None;
                self.close_warning = false;
            }
            Err(error) => self.error = Some(format!("Progress could not be saved: {error}")),
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
            self.persist_progress();
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
            Command::RetrySave => unreachable!(),
        }
    }

    fn shortcut(&self, ctx: &egui::Context) -> Option<Command> {
        if self.dirty || ctx.egui_wants_keyboard_input() {
            return None;
        }
        let game = match self.page {
            Page::Table => &self.table,
            Page::Practice => self.practice.as_ref()?,
            _ => return None,
        };
        ctx.input(|input| {
            if input.modifiers.any() {
                return None;
            }
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
        ctx.request_repaint_after(Duration::from_secs(30));
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
    use tempfile::TempDir;
    use twenty_one_pro::{
        model::{Action, HandKind, Situation},
        training::SCHEMA_VERSION,
    };

    fn app_with_store() -> (TempDir, TrainerApp) {
        let directory = tempfile::tempdir().unwrap();
        let store = Store::open(&directory.path().join("profile.redb")).unwrap();
        let profile = Profile::default();
        let analytics = profile.analytics(Some(StudyMode::Table));
        let app = TrainerApp {
            page: Page::Table,
            profile,
            store: Some(store),
            data_path: directory.path().join("profile.redb"),
            startup_error: None,
            error: None,
            dirty: false,
            allow_close: false,
            close_warning: false,
            table: Game::new(0),
            table_feedback: Vec::new(),
            practice: None,
            practice_feedback: Vec::new(),
            practice_queue: VecDeque::new(),
            practice_total: 0,
            practice_completed: 0,
            practice_start_attempt: 0,
            practice_target: None,
            filter: Some(StudyMode::Table),
            table_stats: analytics.total,
            analytics,
            heatmap: HandKind::Hard,
            selected_cell: None,
            just_unlocked: false,
            fixture: None,
        };
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
