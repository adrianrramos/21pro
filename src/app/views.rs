use std::time::Duration;

use super::{Command, Page, TrainerApp, now, widgets as w};
use eframe::egui::{self, RichText, Stroke, vec2};
use twenty_one_pro::{
    counting::{TRIAL_SIZE, format_completed_at},
    game::Phase,
    model::{ASSESSMENT_ROUNDS, HandKind},
    strategy,
    training::StudyMode,
};

fn format_duration(duration: Duration) -> String {
    let total_seconds = duration.as_secs();
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes:02}:{seconds:02}.{:03}", duration.subsec_millis())
}

impl TrainerApp {
    pub(super) fn sidebar(&mut self, root: &mut egui::Ui) {
        egui::Panel::left("navigation")
            .resizable(false)
            .exact_size(188.0)
            .frame(egui::Frame::NONE.fill(w::SURFACE).inner_margin(20))
            .show(root, |ui| {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("21").size(44.0).strong().color(w::GREEN));
                    ui.label(RichText::new("PRO").size(17.0).strong());
                });
                w::muted(ui, "A better decision.");
                ui.add_space(28.0);
                for (page, number, title) in [
                    (Page::Table, "01", "The table"),
                    (Page::Insights, "02", "Your insights"),
                    (Page::Practice, "03", "Practice"),
                    (Page::Counting, "04", "Card counting"),
                    (Page::Rules, "05", "Rules"),
                ] {
                    let selected = self.page == page;
                    let label = format!("{number}   {title}");
                    let button = egui::Button::new(RichText::new(label).color(if selected {
                        w::GREEN
                    } else {
                        w::MUTED
                    }))
                    .min_size(vec2(ui.available_width(), 44.0))
                    .corner_radius(8)
                    .fill(if selected {
                        egui::Color32::from_rgb(34, 64, 62)
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .stroke(Stroke::NONE);
                    if ui.add(button).clicked() {
                        self.page = page;
                    }
                }
                ui.add_space((ui.available_height() - 274.0).max(24.0));
                w::eyebrow(ui, "YOUR BASELINE");
                ui.label(
                    RichText::new(format!("{} rounds", self.profile.rounds_played()))
                        .size(22.0)
                        .strong(),
                );
                ui.add(
                    egui::ProgressBar::new(
                        (self.profile.rounds_played() as f32 / ASSESSMENT_ROUNDS as f32).min(1.0),
                    )
                    .fill(w::GREEN)
                    .desired_height(5.0),
                );
                ui.label(
                    RichText::new(if self.profile.assessment_unlocked() {
                        "Assessment unlocked".to_owned()
                    } else {
                        format!(
                            "{} until your assessment",
                            ASSESSMENT_ROUNDS.saturating_sub(self.profile.rounds_played())
                        )
                    })
                    .size(12.0)
                    .color(w::MUTED),
                );
                ui.add_space(16.0);
                ui.separator();
                ui.label(RichText::new("YAAMAVA · SIX DECK").size(11.0).strong());
                ui.label(
                    RichText::new("CBJN · February 2025\nOffline · no account needed")
                        .size(11.0)
                        .color(w::MUTED),
                );
                ui.label(
                    RichText::new(if self.dirty {
                        "UNSAVED CHANGES"
                    } else {
                        "PROGRESS SAVED LOCALLY"
                    })
                    .size(10.0)
                    .color(if self.dirty { w::GOLD } else { w::GREEN }),
                );
            });
    }

    pub(super) fn startup_error_view(&self, root: &mut egui::Ui) {
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(w::BG).inner_margin(40)).show(root, |ui| {
            w::heading(ui, "Your progress could not be opened", "The app has not replaced or reset your data.");
            if let Some(error) = &self.startup_error { ui.colored_label(w::GOLD, error); }
            ui.add_space(12.0);
            ui.label(self.data_path.display().to_string());
            w::muted(ui, "If another copy of 21 Pro is running, close it first. Check the file permissions and restart. Keep a backup before repairing or removing an unreadable database.");
            if ui.button("Copy data path").clicked() { ui.ctx().copy_text(self.data_path.display().to_string()); }
            if ui.button("Quit").clicked() { ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close); }
        });
    }

    pub(super) fn table_view(&mut self, ui: &mut egui::Ui, command: &mut Option<Command>) {
        w::heading(
            ui,
            "The table",
            "Six decks. Real decisions. Immediate feedback.",
        );
        ui.columns(3, |columns| {
            w::metric(
                &mut columns[0],
                "COMPLETED ROUNDS",
                &self.profile.rounds_played().to_string(),
                "250 unlocks your assessment",
            );
            w::metric(
                &mut columns[1],
                "TABLE ACCURACY",
                &w::accuracy(self.table_stats),
                "Decisions, not wins",
            );
            w::metric(
                &mut columns[2],
                "CARDS IN SHOE",
                &self.table.remaining_cards().to_string(),
                "Six decks · reshuffle at cut card",
            );
        });
        ui.add_space(16.0);
        w::game_table(ui, &self.table);
        match self.table.phase {
            Phase::Ready => {
                ui.add_space(14.0);
                if w::primary(ui, "Deal first hand  [Enter]").clicked() {
                    *command = Some(Command::Deal);
                }
                w::muted(
                    ui,
                    "Play 250 naturally dealt rounds across as many sessions as you like. We assess each choice against basic strategy, even when a mistake wins.",
                );
            }
            Phase::Finished => {
                ui.add_space(12.0);
                ui.horizontal_wrapped(|ui| {
                    if w::primary(ui, "Next hand  [Enter]").clicked() {
                        *command = Some(Command::Deal);
                    }
                    if let Some(result) = &self.table.result {
                        ui.label(
                            RichText::new(w::units(result.net_half_units))
                                .strong()
                                .color(w::MUTED),
                        );
                        if result.dealer_blackjack {
                            ui.label("Dealer blackjack · original main bet only");
                        }
                    }
                });
                ui.label(RichText::new("Simulated results only. A winning hand can still contain a strategy mistake.").size(12.0).color(w::MUTED));
            }
            _ => w::actions(ui, &self.table, command),
        }
        ui.add_space(14.0);
        ui.label(RichText::new("H hit   S stand   D double   P split   R surrender   I/N insurance   Enter next hand").size(11.0).color(w::MUTED));
    }

    fn locked_view(&mut self, ui: &mut egui::Ui, title: &str) {
        w::heading(
            ui,
            title,
            "First, build a baseline from naturally dealt hands.",
        );
        w::panel().show(ui, |ui| {
            ui.label(RichText::new(format!("{} / {ASSESSMENT_ROUNDS} rounds", self.profile.rounds_played())).size(30.0).strong());
            ui.add(egui::ProgressBar::new(self.profile.rounds_played() as f32 / ASSESSMENT_ROUNDS as f32).fill(w::GREEN));
            ui.add_space(10.0);
            ui.label("Your assessment and personalized practice unlock after 250 completed table rounds, accumulated across sessions.");
            w::muted(ui, "Split hands count as one original round. Practice does not advance this baseline. Your decisions are already being saved.");
            if w::primary(ui, "Back to the table").clicked() { self.page = Page::Table; }
        });
    }

    pub(super) fn insights_view(&mut self, ui: &mut egui::Ui, command: &mut Option<Command>) {
        if !self.profile.assessment_unlocked() {
            self.locked_view(ui, "Your insights");
            return;
        }
        w::heading(
            ui,
            "Know your weak spots",
            "A map of your decisions—not your luck at the table.",
        );
        if self.just_unlocked {
            w::panel().stroke(Stroke::new(1.0, w::GREEN)).show(ui, |ui| {
                ui.label(RichText::new("Baseline complete. Your personalized training is ready.").strong().color(w::GREEN));
                w::muted(ui, "250 rounds is a starting sample, not proof of mastery. Rare hands may still be unseen.");
                ui.horizontal_wrapped(|ui| {
                    if w::primary(ui, "Start focused practice").clicked() { *command = Some(Command::StartPractice(self.profile.practice_queue(now(), 12))); }
                    if ui.button("See my last hand").clicked() { self.page = Page::Table; }
                    if ui.small_button("Dismiss").clicked() { self.just_unlocked = false; }
                });
            });
            ui.add_space(12.0);
        }
        let previous_filter = self.filter;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.filter, Some(StudyMode::Table), "Table play");
            ui.selectable_value(
                &mut self.filter,
                Some(StudyMode::Practice),
                "Focused practice",
            );
            ui.selectable_value(&mut self.filter, None, "All decisions");
        });
        if previous_filter != self.filter {
            self.refresh_analytics();
        }
        ui.add_space(8.0);
        let total = self.analytics.total;
        ui.columns(3, |columns| {
            w::metric(
                &mut columns[0],
                "DECISION ACCURACY",
                &w::accuracy(total),
                "Every first attempt counts",
            );
            w::metric(
                &mut columns[1],
                "DECISIONS OBSERVED",
                &total.attempts.to_string(),
                "Includes each choice after a hit",
            );
            w::metric(
                &mut columns[2],
                "STRATEGY MISTAKES",
                &total.mistakes.to_string(),
                "Independent of the final payout",
            );
        });
        ui.add_space(14.0);
        ui.columns(2, |columns| {
            w::panel().show(&mut columns[0], |ui| w::trend(ui, &self.analytics));
            w::panel().show(&mut columns[1], |ui| w::category_bars(ui, &self.analytics));
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("Your decision heatmap").size(20.0).strong());
            w::muted(
                ui,
                "Cells show mistake rate. Hover for sample size; click to inspect a situation.",
            );
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.heatmap, HandKind::Hard, "Hard totals");
                ui.selectable_value(&mut self.heatmap, HandKind::Soft, "Soft totals");
                ui.selectable_value(&mut self.heatmap, HandKind::Pair, "Pairs");
            });
            if let Some(cell) = w::heatmap(ui, &self.analytics, self.heatmap) {
                self.selected_cell = Some(cell);
            }
            if let Some(cell) = self.selected_cell {
                ui.add_space(10.0);
                ui.separator();
                let mut found = false;
                for skill in self.analytics.weakest.iter().filter(|skill| {
                    (
                        skill.situation.kind,
                        skill.situation.value,
                        skill.situation.dealer,
                    ) == cell
                }) {
                    found = true;
                    let recommendation = strategy::recommendation(skill.situation);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(skill.situation.label()).strong());
                        ui.label(format!(
                            "{} / {} missed · best {}",
                            skill.stats.mistakes,
                            skill.stats.attempts,
                            recommendation.action.label()
                        ));
                        if ui.small_button("Practice").clicked() {
                            *command = Some(Command::StartPractice(vec![skill.situation]));
                        }
                    });
                    ui.label(
                        RichText::new(skill.situation.context())
                            .size(12.0)
                            .color(w::MUTED),
                    );
                    w::muted(ui, recommendation.explanation);
                }
                if !found {
                    w::muted(
                        ui,
                        "No decisions observed in this cell. Unseen is not the same as mastered.",
                    );
                }
            }
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("Where to focus next").size(20.0).strong());
            w::muted(
                ui,
                "Ranked with sample-aware smoothing. A single mistake is a clue, not a verdict.",
            );
            if self.analytics.weakest.is_empty() {
                w::muted(ui, "No observations in this view yet.");
            }
            for skill in self.analytics.weakest.iter().take(8) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(skill.situation.label()).strong());
                    ui.label(format!(
                        "{} / {} missed",
                        skill.stats.mistakes, skill.stats.attempts
                    ));
                    ui.label(
                        RichText::new(w::due_label(skill.due_at, now()))
                            .size(12.0)
                            .color(w::MUTED),
                    );
                    if ui.small_button("Drill").clicked() {
                        *command = Some(Command::StartPractice(vec![skill.situation]));
                    }
                });
                ui.label(
                    RichText::new(skill.situation.context())
                        .size(11.0)
                        .color(w::MUTED),
                );
                ui.separator();
            }
        });
    }

    pub(super) fn practice_view(&mut self, ui: &mut egui::Ui, command: &mut Option<Command>) {
        if !self.profile.assessment_unlocked() {
            self.locked_view(ui, "Focused practice");
            return;
        }
        w::heading(
            ui,
            "Make the hard hands familiar",
            "Targeted hands today. Spaced reviews to remember them tomorrow.",
        );
        if let Some(game) = &self.practice {
            ui.horizontal_wrapped(|ui| {
                w::eyebrow(
                    ui,
                    &format!(
                        "HAND {} OF {}",
                        (self.practice_completed + usize::from(game.phase != Phase::Finished))
                            .max(1),
                        self.practice_total
                    ),
                );
                if let Some(target) = self.practice_target {
                    ui.label(RichText::new(target.label()).strong());
                }
            });
            if let Some(target) = self.practice_target {
                w::muted(ui, target.context());
            }
            ui.add_space(8.0);
            w::game_table(ui, game);
            if game.phase == Phase::Finished {
                ui.add_space(12.0);
                let label = if self.practice_queue.is_empty() {
                    "Finish practice  [Enter]"
                } else {
                    "Next practice hand  [Enter]"
                };
                if w::primary(ui, label).clicked() {
                    *command = Some(Command::NextPractice);
                }
                if let Some(result) = &game.result {
                    w::muted(
                        ui,
                        format!(
                            "{} · simulated outcome, not your score",
                            w::units(result.net_half_units)
                        ),
                    );
                }
            } else {
                w::actions(ui, game, command);
            }
            return;
        }
        if self.practice_total > 0 && self.practice_completed == self.practice_total {
            let attempts = &self.profile.attempts[self.practice_start_attempt..];
            let (correct, total) = attempts
                .iter()
                .filter(|attempt| attempt.mode == StudyMode::Practice)
                .fold((0, 0), |(correct, total), attempt| {
                    (
                        correct + usize::from(attempt.chosen == attempt.expected),
                        total + 1,
                    )
                });
            w::panel().stroke(Stroke::new(1.0, w::GREEN)).show(ui, |ui| {
                ui.label(RichText::new("Practice complete").size(24.0).strong().color(w::GREEN));
                ui.label(format!("{} targeted hands · {correct} / {total} decisions correct", self.practice_completed));
                w::muted(ui, "Your review schedule and progress have been saved. Incorrect answers return sooner; due correct answers earn longer intervals.");
            });
            ui.add_space(14.0);
        }
        let time = now();
        let due = self.profile.due_count(time);
        let next_due = self
            .profile
            .reviews
            .iter()
            .map(|review| review.due_at)
            .min();
        ui.columns(3, |columns| {
            w::metric(
                &mut columns[0],
                "REVIEWS DUE",
                &due.to_string(),
                "Overdue situations come first",
            );
            w::metric(
                &mut columns[1],
                "TRACKED SITUATIONS",
                &self.profile.reviews.len().to_string(),
                "Exact hand + legal actions",
            );
            w::metric(
                &mut columns[2],
                "NEXT REVIEW",
                &w::due_label(next_due, time),
                "Local schedule, no account",
            );
        });
        ui.add_space(16.0);
        w::panel().show(ui, |ui| {
            w::eyebrow(ui, "YOUR PERSONALIZED PLAN");
            ui.label(RichText::new("12 hands. Deliberate practice.").size(24.0).strong());
            ui.label("Start with due reviews, then focus on the situations your own play suggests need attention. Card combinations vary while the underlying decision stays the same.");
            ui.add_space(6.0);
            if w::primary(ui, "Start 12-hand session").clicked() { *command = Some(Command::StartPractice(self.profile.practice_queue(time, 12))); }
            w::muted(ui, "If fewer than 12 situations are known, the plan uses the available ones. Early correct practice does not postpone a scheduled review.");
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("How repetition works").strong());
            ui.label("Miss: review in 10 minutes. Due correct reviews: 1 day, then 6 days, then gradually longer. Miss again and the situation returns to relearning.");
            w::muted(ui, "This uses an SM-2-style scheduler. It is inspired by spaced-repetition study, not Anki synchronization or modern Anki's FSRS algorithm.");
        });
    }

    pub(super) fn counting_view(&mut self, ui: &mut egui::Ui, command: &mut Option<Command>) {
        w::heading(
            ui,
            "Count the shoe",
            "Hi-Lo practice. Six decks. One card at a time.",
        );
        if self.counting.is_none() {
            w::panel().show(ui, |ui| {
                w::eyebrow(ui, "READY WHEN YOU ARE");
                ui.label(RichText::new("52 cards. No hints.").size(24.0).strong());
                ui.label("A fresh six-deck shoe is shuffled for every trial. Cards are dealt without replacement, so identical rank and suit cards can appear.");
                ui.add_space(8.0);
                if w::primary(ui, "Start trial").clicked() {
                    *command = Some(Command::StartCounting);
                }
            });
        } else {
            let (card, seen, complete) = self
                .counting
                .as_ref()
                .map(|trial| {
                    (
                        trial.current_card(),
                        trial.cards_seen(),
                        trial.is_complete(),
                    )
                })
                .unwrap_or((None, 0, false));
            let elapsed = self.counting_duration();
            ui.horizontal(|ui| {
                w::eyebrow(ui, &format!("{seen} / {TRIAL_SIZE} CARDS"));
                ui.label(RichText::new(format_duration(elapsed)).strong());
            });
            ui.add_space(10.0);
            w::panel().show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    if let Some(card) = card {
                        w::counting_card(ui, card);
                    }
                });
            });
            ui.add_space(12.0);
            if self.counting_assessment.is_none() && self.counting_elapsed.is_none() {
                if complete {
                    if w::primary(ui, "Finish").clicked() {
                        *command = Some(Command::FinishCounting);
                    }
                    w::muted(ui, "The final card stays visible until you finish.");
                } else if w::primary(ui, "Next").clicked() {
                    *command = Some(Command::NextCounting);
                }
            } else if let Some(assessment) = &self.counting_assessment {
                let color = if assessment.correct { w::GREEN } else { w::RED };
                w::panel().stroke(Stroke::new(1.0, color)).show(ui, |ui| {
                    ui.label(
                        RichText::new(if assessment.correct {
                            "Correct"
                        } else {
                            "Incorrect"
                        })
                        .size(24.0)
                        .strong()
                        .color(color),
                    );
                    ui.label(format!("Submitted count: {:+}", assessment.submitted));
                    ui.label(format!("Actual count: {:+}", assessment.actual));
                    if assessment.correct {
                        w::muted(ui, "This completed trial was saved.");
                    } else {
                        w::muted(ui, "Incorrect trials are not saved.");
                    }
                    if w::primary(ui, "Start new trial").clicked() {
                        *command = Some(Command::StartCounting);
                    }
                });
            } else {
                w::panel().show(ui, |ui| {
                    w::eyebrow(ui, "FINAL COUNT");
                    ui.label("Enter the running count after the last card.");
                    let response = ui.text_edit_singleline(&mut self.counting_input);
                    if response.lost_focus()
                        && ui.input(|input| input.key_pressed(egui::Key::Enter))
                    {
                        *command = Some(Command::SubmitCounting);
                    }
                    if let Some(error) = &self.counting_input_error {
                        ui.colored_label(w::RED, error);
                    }
                    if w::primary(ui, "Submit count").clicked() {
                        *command = Some(Command::SubmitCounting);
                    }
                });
            }
        }
        ui.add_space(18.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("Saved trials").size(20.0).strong());
            let history = &self.counting_history;
            if history.is_empty() {
                w::muted(
                    ui,
                    "Correct trials will appear here, shortest duration first.",
                );
            } else {
                egui::Grid::new("counting-history")
                    .num_columns(3)
                    .spacing(vec2(24.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Duration").color(w::MUTED));
                        ui.label(RichText::new("Completed").color(w::MUTED));
                        ui.label(RichText::new("Result").color(w::MUTED));
                        ui.end_row();
                        for record in history {
                            ui.label(format_duration(Duration::from_millis(record.duration_ms)));
                            ui.label(format_completed_at(record.completed_at));
                            ui.colored_label(w::GREEN, "Correct");
                            ui.end_row();
                        }
                    });
            }
        });
    }

    pub(super) fn rules_view(&self, ui: &mut egui::Ui) {
        w::heading(
            ui,
            "Rules, not assumptions",
            "A versioned rules snapshot makes every recommendation reproducible.",
        );
        w::panel().show(ui, |ui| {
            w::eyebrow(ui, "YAAMAVA / SIX-DECK BLACKJACK");
            egui::Grid::new("rules").num_columns(2).spacing(vec2(28.0, 10.0)).show(ui, |ui| {
                for (label, value) in [
                    ("Shoe", "6 decks; cut at approximately 1.3 decks remaining"),
                    ("Dealer", "Hits soft 17"),
                    ("Blackjack", "Pays 3:2; split 21 pays ordinary 1:1"),
                    ("Double", "Any first two cards, including after splitting non-aces"),
                    ("Splits", "Up to four hands; resplitting aces allowed"),
                    ("Split aces", "One card each; no hitting or doubling"),
                    ("Surrender", "Late; original two-card hand only"),
                    ("Insurance", "Half a unit, pays 2:1; basic strategy always declines"),
                    ("Strategy", "Total-dependent, H17, 4–8 decks, DAS + surrender"),
                ] { ui.label(RichText::new(label).color(w::MUTED)); ui.label(value); ui.end_row(); }
            });
            ui.add_space(12.0);
            ui.label(RichText::new("Dealer blackjack and surrender timing").strong());
            ui.label("The survey defaults say no look at the hole card and only the original bet lost to dealer blackjack. This simulation reveals the natural at settlement and refunds extra split/double stakes. A surrender request costs half only if the dealer does not have blackjack; otherwise the original main bet is lost. Insurance settles separately.");
            w::muted(ui, "No recommendations use the hidden card, running count, or remaining-shoe composition. Strategy is assessed before the outcome.");
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("Sources & scope").size(20.0).strong());
            ui.label("Current Blackjack News, February 2025, page 13. Yaamava entry dated December 2024. Defaults on page 2; rule key on page 55. This is not a claim about current casino conditions or an affiliation with Yaamava.");
            ui.hyperlink_to("Open the supplied CBJN report", "https://assets.bj21.com/newsletters/pdf_files/000/000/113/original/CBJN2502.pdf?1648784494#page=13");
            ui.hyperlink_to("Strategy reference: Wizard of Odds H17 chart", "https://wizardofodds.com/games/blackjack/strategy/4-decks/");
            w::muted(ui, "The reference page's prose describes S17. This app follows its H17 chart, including the H17-specific doubling and surrender decisions.");
            ui.hyperlink_to("Card artwork: CardMeister full SVG set", "https://cardmeister.github.io/index.html?full");
            w::muted(ui, "The card artwork is bundled offline from CardMeister's full implementation under the Unlicense. Source revision and export provenance are recorded in assets/cards/README.md.");
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("What the statistics mean").size(20.0).strong());
            ui.label("One attempt is one chosen action. Split and hit follow-up decisions count, but only the original completed table round advances the 250-round baseline. Practice results are separately filterable.");
            ui.label("A heatmap cell groups a hand family and dealer upcard. Review cards keep the exact legal-action context: two-card, post-hit, post-split, and split-limit cases are not interchangeable.");
            ui.label("Weakness priority uses (mistakes + 1) / (attempts + 5), a Beta(1,4) prior to temper sparse samples. Displayed heatmap percentages are the actual observed rates. Accuracy trends use consecutive blocks of 25 decisions; the final partial block is marked.");
            w::muted(ui, "Basic strategy reduces avoidable mistakes; it does not remove the casino's edge. This is an educational simulation, with no money, betting services, or promises of profit.");
        });
        ui.add_space(14.0);
        w::panel().show(ui, |ui| {
            ui.label(RichText::new("Your data stays here").strong());
            ui.label(self.data_path.display().to_string());
            if ui.button("Copy data path").clicked() { ui.ctx().copy_text(self.data_path.display().to_string()); }
            w::muted(ui, "Decisions, completed rounds, and review schedules are saved after every move. An unfinished hand is not resumed after closing. Close the app before copying the database as a backup. No telemetry or network service is used; source links open only when clicked.");
        });
    }
}
