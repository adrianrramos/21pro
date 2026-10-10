//! Simulated input against the real eframe app; no renderer or process-global test state.
use super::{Page, TrainerApp, fixtures::Fixture};
use eframe::egui::{Key, accesskit::Role, vec2};
use egui_kittest::{
    Harness,
    kittest::{NodeT, Queryable},
};

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
