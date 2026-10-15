//! Simulated input against the real eframe app; no renderer or process-global test state.
use super::{Command, Page, SuiteActivity, TrainerApp, fixtures::Fixture};
use eframe::egui::{Key, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};
use std::time::{Duration, Instant};
use twenty_one_pro::training_suite::ToolId;

type AppHarness = Harness<'static, TrainerApp>;

fn fixture(scene: Fixture) -> AppHarness {
    Harness::builder()
        .with_size(vec2(1180.0, 860.0))
        .build_eframe(move |cc| TrainerApp::fixture(&cc.egui_ctx, scene).unwrap())
}

fn click(harness: &mut AppHarness, label: &str) {
    harness.get_by_label(label).click();
    harness.run();
}

#[test]
fn table_round_and_natural_both_advance_the_baseline() {
    let mut harness = fixture(Fixture::TableReady);
    click(&mut harness, "Deal first hand  [Enter]");
    assert!(harness.query_by_label("Deal first hand  [Enter]").is_none());
    harness.get_by_label("Stand  [S]");

    harness.key_press(Key::S);
    harness.run();
    harness.get_by_label("Strategy correction");
    harness.get_by_label("1 rounds");
    harness.get_by_label("Next hand  [Enter]");

    // The next seeded deal is a natural: it must count without another decision.
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.query_by_label("Strategy correction").is_none());
    harness.get_by_label("Hand 1: Blackjack · 3:2");
    harness.get_by_label("+1.5 units");
    harness.get_by_label("2 rounds");
}

#[test]
fn unavailable_split_is_ignored_then_legal_surrender_is_scored() {
    let mut harness = fixture(Fixture::TableOpening);
    assert!(
        harness
            .get_by_label("Split  [P]")
            .accesskit_node()
            .is_disabled()
    );
    click(&mut harness, "Split  [P]");
    harness.key_press(Key::P);
    harness.run();
    assert!(harness.query_by_label("Strategy correction").is_none());
    assert!(harness.query_by_label("Correct decision").is_none());
    harness.get_by_label("0 rounds");

    harness.key_press(Key::R);
    harness.run();
    harness.get_by_label("Correct decision");
    harness.get_by_label("1 rounds");
    harness.get_by_label("-0.5 units");
    harness.get_by_label("Next hand  [Enter]");
}

#[test]
fn assessment_and_practice_unlock_only_after_round_250_settles() {
    let mut harness = Harness::builder()
        .with_size(vec2(1180.0, 860.0))
        .build_eframe(|cc| {
            let mut app = TrainerApp::fixture(&cc.egui_ctx, Fixture::Insights).unwrap();
            // Arrange the boundary before the first frame, not by bypassing UI actions.
            app.profile.rounds.pop();
            app.page = Page::Table;
            app.persist_progress();
            app
        });

    click(&mut harness, "02   Your insights");
    harness.get_by_label("249 / 250 rounds");
    assert!(harness.query_by_label("Start focused practice").is_none());
    click(&mut harness, "03   Practice");
    harness.get_by_label("249 / 250 rounds");
    assert!(harness.query_by_label("Start 12-hand session").is_none());
    click(&mut harness, "Back to the table");
    click(&mut harness, "Deal first hand  [Enter]");
    harness.get_by_label("249 rounds");
    click(&mut harness, "Stand  [S]");

    harness.get_by_label("250 rounds");
    harness.get_by_label("Assessment unlocked");
    click(&mut harness, "Start focused practice");
    harness.get_by_label("HAND 1 OF 12");
}

#[test]
fn complete_personalized_practice_without_advancing_table_baseline() {
    let mut harness = fixture(Fixture::Insights);
    click(&mut harness, "03   Practice");
    click(&mut harness, "Start 12-hand session");

    let mut decisions = 0;
    for hand in 1..=12 {
        harness.get_by_label(&format!("HAND {hand} OF 12"));
        if harness.query_by_label("No insurance  [N]").is_some() {
            click(&mut harness, "No insurance  [N]");
            decisions += 1;
        }
        if harness.query_by_label("Stand  [S]").is_some() {
            click(&mut harness, "Stand  [S]");
            decisions += 1;
        }
        click(
            &mut harness,
            if hand == 12 {
                "Finish practice  [Enter]"
            } else {
                "Next practice hand  [Enter]"
            },
        );
    }

    harness.get_by_label("Practice complete");
    harness.get_by_label_contains("12 targeted hands");
    harness.get_by_label_contains(&format!(" / {decisions} decisions correct"));
    harness.get_by_label("250 rounds");
    assert!(harness.query_by_label("Finish practice  [Enter]").is_none());

    click(&mut harness, "01   The table");
    harness.get_by_label("250 rounds");
    harness.get_by_label("Deal first hand  [Enter]");
}

#[test]
fn counting_trial_from_fixture_is_assessed_without_advancing_table_baseline() {
    let mut harness = fixture(Fixture::TableReady);
    harness.get_by_label("0 rounds");
    click(&mut harness, "04   Card counting");
    harness.get_by_label("52 cards. No hints.");
    harness.get_by_label("Start trial").click();
    // The live timer never idles; process input and the following state frame.
    harness.run_steps(2);
    harness.get_by_label("1 / 52 CARDS");
    assert!(harness.query_by_label("Submit count").is_none());

    harness.get_by_label("Next  [Space]").click();
    harness.run_steps(2);
    harness.get_by_label("2 / 52 CARDS");
    for seen in 3..=52 {
        harness.key_press(Key::Space);
        harness.run_steps(2);
        harness.get_by_label(&format!("{seen} / 52 CARDS"));
    }
    assert!(harness.query_by_label("Next  [Space]").is_none());
    assert!(harness.query_by_label("Submit count").is_none());
    harness.get_by_label("Finish  [Space]");
    harness.key_press(Key::Space);
    harness.run();
    harness.get_by_label("FINAL COUNT");
    harness.get_by_label("52 / 52 CARDS");

    click(&mut harness, "Submit count");
    harness.get_by_label("Enter a signed whole number, such as +3 or -2.");
    assert!(harness.query_by_label("Incorrect").is_none());
    harness.get_by_role(Role::TextInput).click();
    harness.run();
    // Each of 52 cards contributes at most +1: +53 is wrong for every shuffle.
    harness.get_by_role(Role::TextInput).type_text("+53");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    harness.get_by_label("Incorrect");
    harness.get_by_label("Submitted count: +53");
    harness.get_by_label("Incorrect trials are not saved.");
    harness.get_by_label("Correct trials will appear here, shortest duration first.");
    assert!(harness.query_by_label("Submit count").is_none());
    assert!(
        harness
            .query_by_label("Enter a signed whole number, such as +3 or -2.")
            .is_none()
    );

    harness.get_by_label("Start new trial").click();
    harness.run_steps(2);
    harness.get_by_label("1 / 52 CARDS");
    harness.get_by_label("Next  [Space]");
    assert!(harness.query_by_label("Incorrect").is_none());
    click(&mut harness, "01   The table");
    harness.get_by_label("0 rounds");
    harness.get_by_label("Deal first hand  [Enter]");
    click(&mut harness, "02   Your insights");
    harness.get_by_label("0 / 250 rounds");
    assert!(harness.query_by_label("Start focused practice").is_none());
}

fn active_suite(tool: ToolId) -> AppHarness {
    Harness::builder()
        .with_size(vec2(1180.0, 860.0))
        .build_eframe(move |cc| {
            let mut app = TrainerApp::fixture(&cc.egui_ctx, Fixture::TrainingSuite).unwrap();
            app.suite_seed = 7;
            app.execute(Command::StartSuite(tool));
            app
        })
}

#[test]
fn training_suite_menu_exposes_cards_settings_and_current_lifetime_stats() {
    let harness = fixture(Fixture::TrainingSuite);
    assert!(
        harness
            .query_all_by_label("CURRENT ACCURACY")
            .next()
            .is_some()
    );
    assert!(
        harness
            .query_all_by_label("LIFETIME ACCURACY")
            .next()
            .is_some()
    );
    harness.get_by_label("End of shoe");
    harness.get_by_label("1 round");
    harness.get_by_label("Training Stats");
    harness.get_by_label("Start Running Count");
    harness.get_by_label("Start True Count");
}

#[test]
fn running_count_active_surface_shows_automatic_table_and_preferences() {
    let harness = active_suite(ToolId::RunningCount);
    harness.get_by_label("DEALER");
    assert!(harness.query_by_label("DISCARD TRAY").is_none());
    harness.get_by_label("AUTOMATIC DEAL");
    harness.get_by_label("PREFERENCES");
    harness.get_by_label("Dealing speed: 6");
    harness.get_by_label_contains("Penetration");
}
#[test]
fn running_count_auto_deals_and_grades_a_wrong_count() {
    let ctx = eframe::egui::Context::default();
    let mut app = TrainerApp::fixture(&ctx, Fixture::TrainingSuite).unwrap();
    app.suite_seed = 7;
    app.execute(Command::StartSuite(ToolId::RunningCount));

    let mut now = Instant::now() + Duration::from_secs(10);
    let mut prompted = false;
    for _ in 0..100 {
        app.tick_running_count(now);
        if matches!(
            app.suite.as_ref(),
            Some(SuiteActivity::RunningCount(drill)) if drill.checkpoint.prompt
        ) {
            prompted = true;
            break;
        }
        now += Duration::from_secs(1);
    }
    assert!(prompted, "automatic round should reach a count prompt");

    let expected = match app.suite.as_ref() {
        Some(SuiteActivity::RunningCount(drill)) => drill.running_count(),
        _ => panic!("Running Count should remain active at the prompt"),
    };
    app.suite_input = (expected + 1).to_string();
    app.execute(Command::SuiteSubmit);
    assert_eq!(app.suite_attempts, 1);
    assert_eq!(app.suite_correct, 0);
    assert!(
        app.suite_message
            .as_deref()
            .is_some_and(|message| message.contains(&format!("{expected:+}")))
    );
}
#[test]
fn running_count_speed_setting_persists_and_rejects_invalid_labels() {
    let ctx = eframe::egui::Context::default();
    let mut app = TrainerApp::fixture(&ctx, Fixture::TrainingSuite).unwrap();

    app.execute(Command::SuiteSetDealingSpeed(0));
    assert_eq!(app.profile.suite_settings.dealing_delay_ms, 2_000);
    app.execute(Command::SuiteSetDealingSpeed(11));
    assert_eq!(app.profile.suite_settings.dealing_delay_ms, 2_000);
    assert_eq!(
        app.store
            .as_ref()
            .unwrap()
            .load()
            .unwrap()
            .suite_settings
            .dealing_delay_ms,
        2_000
    );
}

#[test]
fn running_count_pauses_then_clears_before_next_deal() {
    let ctx = eframe::egui::Context::default();
    let mut app = TrainerApp::fixture(&ctx, Fixture::TrainingSuite).unwrap();
    app.profile.suite_settings.count_every = None;
    app.suite_seed = 7;
    app.execute(Command::StartSuite(ToolId::RunningCount));

    let mut now = Instant::now() + Duration::from_secs(10);
    let clear_at = (0..200)
        .find_map(|_| {
            app.tick_running_count(now);
            if let Some(clear_at) = app.suite_running_clear_at {
                Some(clear_at)
            } else {
                now += Duration::from_secs(1);
                None
            }
        })
        .expect("automatic round should schedule a clear pause");
    assert_eq!(clear_at, now + Duration::from_secs(3));
    assert!(matches!(
        app.suite.as_ref(),
        Some(SuiteActivity::RunningCount(drill)) if drill.cards_visible()
    ));

    app.tick_running_count(now + Duration::from_secs(2));
    assert!(matches!(
        app.suite.as_ref(),
        Some(SuiteActivity::RunningCount(drill)) if drill.cards_visible()
    ));

    app.tick_running_count(clear_at);
    assert!(matches!(
        app.suite.as_ref(),
        Some(SuiteActivity::RunningCount(drill)) if !drill.cards_visible()
    ));
    let next_deal_at = app
        .suite_running_next_step_at
        .expect("clearing schedules the next deal");
    app.tick_running_count(next_deal_at);
    assert!(matches!(
        app.suite.as_ref(),
        Some(SuiteActivity::RunningCount(drill)) if drill.cards_visible()
    ));
    assert!(app.running_count_card_progress(next_deal_at) <= f32::EPSILON);
}

#[test]
fn true_count_active_surface_exposes_tray_angles_and_answer_card() {
    let mut harness = active_suite(ToolId::TrueCount);
    harness.get_by_label("DISCARD TRAY");
    harness.get_by_label("TRAY ANGLE");
    harness.get_by_label("ENTER THE TRUE COUNT");
    harness.get_by_label("›").click();
    harness.run_steps(2);
}

#[test]
fn basic_strategy_active_surface_shows_cards_and_actions() {
    let harness = active_suite(ToolId::BasicStrategy);
    harness.get_by_label("DEALER");
    harness.get_by_label("DECISION CARD");
    harness.get_by_label("Hit [H]");
}

#[test]
fn core_index_active_surface_shows_cards_and_threshold_input() {
    let harness = active_suite(ToolId::CoreIndexPlays);
    harness.get_by_label("DEALER");
    harness.get_by_label("INDEX DECISION");
    harness.get_by_label("True-count threshold");
}

#[test]
fn pressure_table_active_surface_shows_ghost_and_decision_clock() {
    let harness = active_suite(ToolId::PressureTable);
    harness.get_by_label("PRESSURE FEED");
    harness.get_by_label("Ghost seat occupied");
    harness.get_by_label_contains("Decision clock");
    harness.get_by_label("Hit [H]");
}

#[test]
fn ending_a_suite_session_records_incomplete_without_a_finished_report() {
    let ctx = eframe::egui::Context::default();
    let mut app = TrainerApp::fixture(&ctx, Fixture::TrainingSuite).unwrap();
    app.execute(Command::StartSuite(ToolId::RunningCount));
    app.execute(Command::SuiteEnd);
    assert!(app.suite_report.is_none());
    let session = app
        .profile
        .suite_sessions
        .last()
        .expect("session is persisted");
    assert!(!session.finished);
    assert_eq!(session.passed, None);
}
