//! Transactional redb storage of one versioned JSON profile. Errors never reset
//! progress; callers must show the error rather than replace an unreadable file.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use redb::{Database, ReadableDatabase, TableDefinition};
use thiserror::Error;

use crate::model::{RULESET_ID, Situation};
use crate::strategy::recommendation;
use crate::training::{Profile, SCHEMA_VERSION, valid_situation};

const PROFILE: TableDefinition<&str, &[u8]> = TableDefinition::new("profile");
const PROFILE_KEY: &str = "current";

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Cannot determine the application data directory")]
    NoDataDirectory,
    #[error("Profile file is locked by another running instance; close it before trying again")]
    Locked,
    #[error("Profile I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Profile database error: {0}")]
    Database(String),
    #[error("Corrupt profile: {0}. The file has not been reset")]
    Corrupt(String),
    #[error("Unsupported profile schema {found}; this application supports {supported}")]
    Version { found: u64, supported: u32 },
    #[error("Profile ruleset {found:?} does not match {expected:?}")]
    Ruleset {
        found: String,
        expected: &'static str,
    },
}

pub struct Store {
    database: Database,
}

/// macOS: ~/Library/Application Support/dev.TwentyOnePro.21Pro/profile.redb.
/// Other platforms use their standard per-user local application data directory.
pub fn default_path() -> Result<PathBuf, StorageError> {
    let dirs =
        ProjectDirs::from("dev", "TwentyOnePro", "21Pro").ok_or(StorageError::NoDataDirectory)?;
    Ok(dirs.data_local_dir().join("profile.redb"))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        match std::fs::metadata(path) {
            Ok(metadata) if metadata.len() == 0 => {
                return Err(StorageError::Corrupt(
                    "existing database file is empty".to_owned(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let database = Database::create(path).map_err(|error| match error {
            redb::DatabaseError::DatabaseAlreadyOpen => StorageError::Locked,
            redb::DatabaseError::Storage(redb::StorageError::Corrupted(message)) => {
                StorageError::Corrupt(message)
            }
            other => database_error(other),
        })?;
        Ok(Self { database })
    }

    pub fn load(&self) -> Result<Profile, StorageError> {
        let read = self.database.begin_read().map_err(database_error)?;
        let table = match read.open_table(PROFILE) {
            Ok(table) => table,
            Err(redb::TableError::TableDoesNotExist(_)) => {
                // Only a genuinely empty database is a new profile. A populated
                // unrelated database must not be mistaken for blank progress.
                let has_tables = read.list_tables().map_err(database_error)?.next().is_some();
                let has_multimaps = read
                    .list_multimap_tables()
                    .map_err(database_error)?
                    .next()
                    .is_some();
                if has_tables || has_multimaps {
                    return Err(StorageError::Corrupt("profile table is missing".to_owned()));
                }
                return Ok(Profile::default());
            }
            Err(error) => return Err(database_error(error)),
        };
        let snapshot = table
            .get(PROFILE_KEY)
            .map_err(database_error)?
            .ok_or_else(|| StorageError::Corrupt("profile snapshot is missing".to_owned()))?;
        let value: serde_json::Value = serde_json::from_slice(snapshot.value())
            .map_err(|error| StorageError::Corrupt(error.to_string()))?;
        let version = value
            .get("schema_version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                StorageError::Corrupt("schema_version is missing or invalid".to_owned())
            })?;
        check_version(version)?;
        let profile: Profile = serde_json::from_value(value)
            .map_err(|error| StorageError::Corrupt(error.to_string()))?;
        validate(&profile)?;
        Ok(profile)
    }

    /// Serialize before entering a write transaction. Commit atomically replaces
    /// the single snapshot; a rejected profile leaves the previous one intact.
    pub fn save(&self, profile: &Profile) -> Result<(), StorageError> {
        // ponytail: one atomic snapshot suits a personal history; use append-only
        // tables if long histories cause measurable save pauses.
        validate(profile)?;
        let snapshot = serde_json::to_vec(profile)
            .map_err(|error| StorageError::Corrupt(error.to_string()))?;
        let write = self.database.begin_write().map_err(database_error)?;
        {
            let mut table = write.open_table(PROFILE).map_err(database_error)?;
            table
                .insert(PROFILE_KEY, snapshot.as_slice())
                .map_err(database_error)?;
        }
        write.commit().map_err(database_error)?;
        Ok(())
    }
}

fn database_error(error: impl std::fmt::Display) -> StorageError {
    StorageError::Database(error.to_string())
}

fn check_version(version: u64) -> Result<(), StorageError> {
    if version != u64::from(SCHEMA_VERSION) {
        return Err(StorageError::Version {
            found: version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(())
}

fn validate(profile: &Profile) -> Result<(), StorageError> {
    check_version(u64::from(profile.schema_version))?;
    if profile.ruleset_id != RULESET_ID {
        return Err(StorageError::Ruleset {
            found: profile.ruleset_id.clone(),
            expected: RULESET_ID,
        });
    }
    let corrupt = |message: &str| StorageError::Corrupt(message.to_owned());
    if profile.rounds.iter().any(|round| round.at < 0)
        || profile
            .counting_history
            .iter()
            .any(|trial| trial.completed_at < 0)
    {
        return Err(corrupt("negative completion timestamp"));
    }
    // Keep validation linearithmic, not a replay of every scheduling update.
    let mut counts = BTreeMap::<Situation, (u32, u32)>::new();
    let mut decision_times = BTreeSet::new();
    for attempt in &profile.attempts {
        if attempt.at < 0
            || !valid_situation(attempt.situation)
            || !attempt.situation.allows(attempt.chosen)
            || recommendation(attempt.situation).action != attempt.expected
        {
            return Err(corrupt(
                "invalid decision, legal actions, expected strategy, or timestamp",
            ));
        }
        let (correct, wrong) = counts.entry(attempt.situation).or_default();
        if attempt.chosen == attempt.expected {
            *correct = correct.saturating_add(1);
        } else {
            *wrong = wrong.saturating_add(1);
        }
        decision_times.insert((attempt.situation, attempt.at));
    }
    for review in &profile.reviews {
        let (correct, wrong) = counts.remove(&review.situation).unwrap_or_default();
        let interval_valid = match review.repetitions {
            0 => review.interval_days == 0,
            1 => review.interval_days == 1,
            2 => review.interval_days == 6,
            _ => review.interval_days >= 6,
        };
        let delay = if review.repetitions == 0 {
            600
        } else {
            i64::from(review.interval_days) * 86_400
        };
        if !valid_situation(review.situation)
            || !review.ease.is_finite()
            || !(1.3..=2.5).contains(&review.ease)
            || wrong == 0
            || review.lapses != wrong
            || review.repetitions > correct
            || !interval_valid
            || review.last_review_at < 0
            || review.due_at != review.last_review_at.saturating_add(delay)
            || !decision_times.contains(&(review.situation, review.last_review_at))
        {
            return Err(corrupt("invalid or duplicate review schedule"));
        }
    }
    if counts.values().any(|(_, wrong)| *wrong > 0) {
        return Err(corrupt(
            "a mistaken situation is missing its review schedule",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Action, HandKind};
    use crate::training::StudyMode;

    fn situation() -> Situation {
        Situation {
            kind: HandKind::Hard,
            value: 16,
            dealer: 10,
            can_double: true,
            can_split: false,
            can_surrender: true,
            split_aces: false,
        }
    }

    fn raw_snapshot(store: &Store, json: &[u8]) {
        let write = store.database.begin_write().unwrap();
        {
            let mut table = write.open_table(PROFILE).unwrap();
            table.insert(PROFILE_KEY, json).unwrap();
        }
        write.commit().unwrap();
    }

    #[test]
    fn on_disk_roundtrip_preserves_contexts_progress_and_reviews() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/profile.redb");
        let store = Store::open(&path).unwrap();
        let mut profile = store.load().unwrap();
        assert_eq!(profile.rounds_played(), 0);
        let original = situation();
        let after_hit = Situation {
            can_double: false,
            can_surrender: false,
            ..original
        };
        for i in 0..250 {
            profile.record_round(i, if i % 2 == 0 { 3 } else { -2 });
        }
        profile.record_attempt(original, Action::Stand, 1000, StudyMode::Table);
        profile.record_attempt(after_hit, Action::Stand, 1100, StudyMode::Practice);
        profile.record_counting_trial(1200, 3456);
        store.save(&profile).unwrap();
        let before = serde_json::to_value(&profile).unwrap();
        drop(store);
        let reopened = Store::open(&path).unwrap();
        let loaded = reopened.load().unwrap();
        assert_eq!(serde_json::to_value(&loaded).unwrap(), before);
        assert_eq!(loaded.rounds_played(), 250);
        assert!(loaded.assessment_unlocked());
        assert_eq!(loaded.reviews.len(), 2);
        assert_eq!(loaded.counting_history.len(), 1);
        assert_eq!(loaded.counting_history[0].duration_ms, 3456);
        assert_eq!(loaded.due_count(1600), 1);
        assert_eq!(loaded.practice_queue(1800, 10), vec![original, after_hit]);
        assert_eq!(
            loaded.analytics(None).cells[&(HandKind::Hard, 16, 10)].attempts,
            2
        );
    }

    #[test]
    fn older_profiles_without_counting_history_still_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.redb")).unwrap();
        let mut snapshot = serde_json::to_value(Profile::default()).unwrap();
        snapshot.as_object_mut().unwrap().remove("counting_history");
        raw_snapshot(&store, &serde_json::to_vec(&snapshot).unwrap());
        assert!(store.load().unwrap().counting_history.is_empty());
    }

    #[test]
    fn negative_counting_completion_is_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.redb")).unwrap();
        let mut profile = Profile::default();
        profile
            .counting_history
            .push(crate::counting::CountingRecord {
                completed_at: -1,
                duration_ms: 100,
            });
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
    }

    #[test]
    fn invalid_json_version_ruleset_and_records_are_reported_without_reset() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.redb")).unwrap();
        raw_snapshot(&store, b"{broken");
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
        raw_snapshot(&store, br#"{"schema_version":99}"#);
        assert!(matches!(
            store.load(),
            Err(StorageError::Version { found: 99, .. })
        ));
        let mut profile = Profile {
            ruleset_id: "different-rules".to_owned(),
            ..Profile::default()
        };
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Ruleset { .. })));
        profile.ruleset_id = RULESET_ID.to_owned();
        profile.record_attempt(situation(), Action::Stand, 100, StudyMode::Table);
        profile.attempts[0].expected = Action::Insure;
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
        profile.attempts[0].expected = recommendation(situation()).action;
        profile.reviews[0].due_at = -1;
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
        profile.reviews[0].due_at = 700;
        profile.reviews.push(profile.reviews[0].clone());
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
        profile.reviews.clear();
        raw_snapshot(&store, &serde_json::to_vec(&profile).unwrap());
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
    }

    #[test]
    fn rejected_save_preserves_previous_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.redb")).unwrap();
        let mut p = Profile::default();
        p.record_round(100, 3);
        store.save(&p).unwrap();
        p.schema_version += 1;
        assert!(matches!(store.save(&p), Err(StorageError::Version { .. })));
        assert_eq!(store.load().unwrap().rounds_played(), 1);
    }

    #[test]
    fn locked_and_damaged_files_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.redb");
        let store = Store::open(&path).unwrap();
        assert!(matches!(Store::open(&path), Err(StorageError::Locked)));
        drop(store);
        let damaged = dir.path().join("damaged.redb");
        std::fs::write(&damaged, b"not a redb database").unwrap();
        assert!(Store::open(&damaged).is_err());
        assert_eq!(std::fs::read(&damaged).unwrap(), b"not a redb database");
        let empty = dir.path().join("empty.redb");
        std::fs::write(&empty, []).unwrap();
        assert!(matches!(Store::open(&empty), Err(StorageError::Corrupt(_))));
    }

    #[test]
    fn missing_snapshot_is_corruption_not_blank_progress() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("profile.redb")).unwrap();
        let write = store.database.begin_write().unwrap();
        write.open_table(PROFILE).unwrap();
        write.commit().unwrap();
        assert!(matches!(store.load(), Err(StorageError::Corrupt(_))));
    }
}
