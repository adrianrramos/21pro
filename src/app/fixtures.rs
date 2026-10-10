//! Opt-in, disposable visual scenes built with the real engine and persistence.
use super::{Command, Page, TrainerApp};
use eframe::egui;
use rand::{Rng, SeedableRng, rngs::SmallRng};
use std::{error::Error, str::FromStr};
use twenty_one_pro::{
    game::{Game, GameError},
    model::{ASSESSMENT_ROUNDS, Action, HandKind, Situation},
    storage::Store,
    strategy,
    training::{Profile, StudyMode},
};

pub(super) const NOW: i64 = 1_800_000_000;
const SEED: u64 = 21;

pub(super) struct Session {
    rng: SmallRng,
    _directory: tempfile::TempDir,
}

impl Session {
    pub(super) fn seed(&mut self) -> u64 {
        self.rng.random()
    }
}

#[derive(Clone, Copy)]
pub(super) enum Fixture {
    TableReady,
    TableOpening,
    TableFeedback,
    Insights,
    Practice,
    Rules,
}

impl FromStr for Fixture {
    type Err = &'static str;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        match name {
            "table-ready" => Ok(Self::TableReady),
            "table-opening" => Ok(Self::TableOpening),
            "table-feedback" => Ok(Self::TableFeedback),
            "insights" => Ok(Self::Insights),
            "practice" => Ok(Self::Practice),
            "rules" => Ok(Self::Rules),
            _ => Err(
                "Unknown visual fixture; choose table-ready, table-opening, table-feedback, insights, practice, or rules",
            ),
        }
    }
}

impl Fixture {
    #[cfg(feature = "dev-fixtures")]
    pub(super) fn from_env() -> Result<Option<Self>, Box<dyn Error + Send + Sync>> {
        match std::env::var("TWENTY_ONE_PRO_FIXTURE") {
            Ok(name) => Ok(Some(name.parse()?)),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }
}

impl TrainerApp {
    pub(super) fn fixture(
        ctx: &egui::Context,
        fixture: Fixture,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        // Never inspect the user's data path, even when it is explicitly overridden.
        let directory = tempfile::tempdir()?;
        let data_path = directory.path().join("profile.redb");
        let store = Store::open(&data_path)?;
        let profile = if matches!(
            fixture,
            Fixture::Insights | Fixture::Practice | Fixture::Rules
        ) {
            sample_profile()?
        } else {
            Profile::default()
        };
        store.save(&profile)?;
        let table = if matches!(fixture, Fixture::TableOpening | Fixture::TableFeedback) {
            Game::practice(
                Situation {
                    kind: HandKind::Hard,
                    value: 16,
                    dealer: 10,
                    can_double: true,
                    can_split: false,
                    can_surrender: true,
                    split_aces: false,
                },
                SEED,
            )?
        } else {
            Game::new(SEED)
        };
        let mut app = Self::with_profile(ctx, profile, Some(store), data_path, table);
        app.fixture = Some(Session {
            rng: SmallRng::seed_from_u64(SEED),
            _directory: directory,
        });
        match fixture {
            Fixture::TableReady | Fixture::TableOpening => {}
            Fixture::TableFeedback => app.execute(Command::Act(Action::Stand)),
            Fixture::Insights => app.page = Page::Insights,
            Fixture::Practice => {
                app.execute(Command::StartPractice(app.profile.practice_queue(NOW, 12)));
            }
            Fixture::Rules => app.page = Page::Rules,
        }
        if let Some(error) = &app.error {
            return Err(error.clone().into());
        }
        Ok(app)
    }
}

fn sample_profile() -> Result<Profile, GameError> {
    let mut profile = Profile::default();
    let mut game = Game::new(SEED);
    for round in 0..ASSESSMENT_ROUNDS {
        let at = NOW - 86_400 + round as i64 * 120;
        game.deal()?;
        while let Some(situation) = game.situation() {
            let expected = strategy::recommendation(situation).action;
            // Include real, legal mistakes so heatmaps and due reviews are populated.
            let chosen = if profile.attempts.len().is_multiple_of(4) {
                Action::ALL
                    .into_iter()
                    .find(|&action| action != expected && situation.allows(action))
                    .unwrap_or(expected)
            } else {
                expected
            };
            game.act(chosen)?;
            profile.record_attempt(situation, chosen, at, StudyMode::Table);
        }
        let result = game.result.as_ref().expect("fixture round must settle");
        profile.record_round(at, result.net_half_units);
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_fixture_is_rejected() {
        assert!("".parse::<Fixture>().is_err());
        assert!("personal-profile".parse::<Fixture>().is_err());
    }

    #[test]
    fn fixture_progress_is_saved_but_discarded_between_runs() {
        let ctx = egui::Context::default();
        let mut app = TrainerApp::fixture(&ctx, Fixture::TableOpening).unwrap();
        let path = app.data_path.clone();
        app.execute(Command::Act(Action::Stand));
        let saved = app.store.as_ref().unwrap().load().unwrap();
        assert_eq!(saved.rounds_played(), 1);
        assert_eq!(saved.attempts[0].chosen, Action::Stand);
        assert_eq!(saved.attempts[0].at, NOW);
        assert!(!app.dirty);
        drop(app);
        assert!(!path.exists());

        let fresh = TrainerApp::fixture(&ctx, Fixture::TableOpening).unwrap();
        assert_eq!(fresh.profile.rounds_played(), 0);
        assert!(fresh.profile.attempts.is_empty());
    }
}
