//! Desktop orchestration: render from state, then apply at most one user command.
//! The game engine never depends on egui, and progress is saved after each decision.
mod views;
mod widgets;

use eframe::egui;
use std::{
    collections::VecDeque,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use twenty_one_pro::{
    counting::{CountingTrial, parse_submitted_count},
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
    correct: bool,
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
    filter: Option<StudyMode>,
    analytics: Analytics,
    table_stats: CellStats,
    heatmap: HandKind,
    selected_cell: Option<(HandKind, u8, u8)>,
    just_unlocked: bool,
}

pub(super) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock must be after 1970")
        .as_secs() as i64
}

impl TrainerApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        widgets::configure(&cc.egui_ctx);
        let mut startup_error = None;
        let mut store = None;
        let mut profile = Profile::default();
        let path = std::env::var_os("TWENTY_ONE_PRO_DATA_DIR")
            .map(|directory| Ok(PathBuf::from(directory).join("profile.redb")))
            .unwrap_or_else(storage::default_path);
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
        let analytics = profile.analytics(Some(StudyMode::Table));
        let table_stats = analytics.total;
        Self {
            page: Page::Table,
            profile,
            store,
            data_path,
            startup_error,
            error: None,
            dirty: false,
            allow_close: false,
            close_warning: false,
            table: Game::new(rand::random()),
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
            filter: Some(StudyMode::Table),
            analytics,
            table_stats,
            heatmap: HandKind::Hard,
            selected_cell: None,
            just_unlocked: false,
        }
    }

    fn counting_duration(&self) -> Duration {
        self.counting_elapsed
            .or_else(|| self.counting_started_at.map(|started| started.elapsed()))
            .unwrap_or_default()
    }

    fn reset_counting_trial(&mut self) {
        let mut trial = CountingTrial::new(rand::random());
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
            self.profile.record_round(now(), result.net_half_units);
            if !was_unlocked && self.profile.assessment_unlocked() {
                self.just_unlocked = true;
                self.page = Page::Insights;
            }
        }
    }

    fn start_next_practice(&mut self) {
        if let Some(target) = self.practice_queue.front().copied() {
            match Game::practice(target, rand::random()) {
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
                self.profile.record_attempt(situation, action, now(), mode);
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
                self.counting_assessment = Some(CountingAssessment {
                    submitted,
                    actual,
                    correct,
                });
                if correct {
                    self.profile
                        .record_counting_trial(now(), elapsed.as_millis() as u64);
                    self.persist_progress();
                }
            }
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
        ctx.request_repaint_after(if self.counting_started_at.is_some() {
            Duration::from_millis(100)
        } else {
            Duration::from_secs(30)
        });
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
