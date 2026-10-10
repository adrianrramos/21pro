//! Offline bankroll variance simulation independent of the desktop renderer.
//!
//! Analytical lines use the unrounded Gaussian inputs. Player paths round each
//! hourly result to a whole dollar and freeze after their first non-positive
//! balance; therefore a path can diverge from the EV line even when SD is zero.

use rand::{Rng, SeedableRng, rngs::SmallRng};

/// Educational reference EV default in dollars per hour.
pub const DEFAULT_HOURLY_EV: f64 = 18.93;
/// Educational reference standard deviation default in dollars per hour.
pub const DEFAULT_HOURLY_SD: f64 = 330.97;
/// Starting bankroll default in dollars.
pub const DEFAULT_BANKROLL: f64 = 10_000.0;
/// Simulation duration default in hours.
pub const DEFAULT_HOURS: usize = 500;
/// Number of displayed player paths by default.
pub const DEFAULT_PLAYERS: usize = 1;

/// Validated stochastic inputs. Monetary values are dollars and accept cents precision.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inputs {
    /// Expected dollar result per hour.
    pub hourly_ev: f64,
    /// Dollar standard deviation of one hour.
    pub hourly_sd: f64,
    /// Non-negative starting bankroll in dollars.
    pub starting_bankroll: f64,
    /// Number of simulated hours.
    pub hours: usize,
    /// Number of independent displayed paths.
    pub players: usize,
}

impl Inputs {
    /// Validates finite, cent-precision monetary values and the bounded simulation dimensions.
    pub fn validate(self) -> Result<Self, &'static str> {
        let cents = |value: f64| ((value * 100.0).round() - value * 100.0).abs() < 1e-9;
        if !self.hourly_ev.is_finite()
            || !cents(self.hourly_ev)
            || !(-1_000_000.0..=1_000_000.0).contains(&self.hourly_ev)
        {
            return Err(
                "Hourly EV must be finite, use cents precision, and be between -$1,000,000 and $1,000,000.",
            );
        }
        if !self.hourly_sd.is_finite()
            || !cents(self.hourly_sd)
            || !(0.0..=1_000_000.0).contains(&self.hourly_sd)
        {
            return Err(
                "Hourly standard deviation must be finite, use cents precision, and be between $0 and $1,000,000.",
            );
        }
        if !self.starting_bankroll.is_finite()
            || !cents(self.starting_bankroll)
            || !(0.0..=1_000_000_000.0).contains(&self.starting_bankroll)
        {
            return Err(
                "Starting bankroll must be finite, use cents precision, and be between $0 and $1,000,000,000.",
            );
        }
        if !(1..=10_000).contains(&self.hours) {
            return Err("Hours must be a whole number from 1 through 10,000.");
        }
        if !(1..=9).contains(&self.players) {
            return Err("Players simulated must be an integer from 1 through 9.");
        }
        Ok(self)
    }
}

/// Analytical mean and reference standard-deviation bands for one hour.
#[derive(Clone, Debug)]
pub struct AnalyticalPoint {
    /// Unstopped EV reference.
    pub mean: f64,
    /// Mean plus one accumulated SD.
    pub one_above: f64,
    /// Mean minus one accumulated SD.
    pub one_below: f64,
    /// Mean plus two accumulated SD.
    pub two_above: f64,
    /// Mean minus two accumulated SD.
    pub two_below: f64,
}

/// Complete deterministic result for one validated input snapshot and seed.
#[derive(Clone, Debug)]
pub struct SimulationResult {
    /// Inputs captured by this result.
    pub inputs: Inputs,
    /// Seed used for player-path sampling.
    pub seed: u64,
    /// H+1 analytical samples, including hour zero.
    pub analytical: Vec<AnalyticalPoint>,
    /// One H+1 rounded/stopped balance path per player.
    pub paths: Vec<Vec<f64>>,
    /// First non-positive hour for each path, or `None`.
    pub depleted_hours: Vec<Option<usize>>,
}

impl SimulationResult {
    /// Simulates independent paths with a Marsaglia-polar standard normal sampler.
    ///
    /// Returns an error before allocation when inputs violate the validation bounds.
    pub fn simulate(inputs: Inputs, seed: u64) -> Result<Self, &'static str> {
        let inputs = inputs.validate()?;
        let samples = inputs.hours + 1;
        let mut analytical = Vec::with_capacity(samples);
        for hour in 0..=inputs.hours {
            let h = hour as f64;
            let mean = inputs.starting_bankroll + inputs.hourly_ev * h;
            let sd = inputs.hourly_sd * h.sqrt();
            analytical.push(AnalyticalPoint {
                mean,
                one_above: mean + sd,
                one_below: mean - sd,
                two_above: mean + 2.0 * sd,
                two_below: mean - 2.0 * sd,
            });
        }

        let mut rng = SmallRng::seed_from_u64(seed);
        let mut normal = NormalSampler::default();
        let mut paths = Vec::with_capacity(inputs.players);
        let mut depleted_hours = Vec::with_capacity(inputs.players);
        for _ in 0..inputs.players {
            let mut balance = inputs.starting_bankroll;
            let mut path = Vec::with_capacity(samples);
            path.push(balance);
            let mut depleted = (balance <= 0.0).then_some(0);
            for hour in 1..=inputs.hours {
                if balance > 0.0 {
                    let z = normal.next(&mut rng);
                    let hourly_result = (inputs.hourly_ev + inputs.hourly_sd * z + 0.5).floor();
                    balance += hourly_result;
                    if balance <= 0.0 && depleted.is_none() {
                        depleted = Some(hour);
                    }
                }
                path.push(balance);
            }
            paths.push(path);
            depleted_hours.push(depleted);
        }

        Ok(Self {
            inputs,
            seed,
            analytical,
            paths,
            depleted_hours,
        })
    }
}

#[derive(Default)]
struct NormalSampler {
    cached: Option<f64>,
}

impl NormalSampler {
    fn next(&mut self, rng: &mut SmallRng) -> f64 {
        if let Some(value) = self.cached.take() {
            return value;
        }
        loop {
            let u: f64 = rng.random_range(-1.0..1.0);
            let v: f64 = rng.random_range(-1.0..1.0);
            let s = u * u + v * v;
            if s == 0.0 || s >= 1.0 {
                continue;
            }
            let scale = (-2.0 * s.ln() / s).sqrt();
            self.cached = Some(v * scale);
            return u * scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(bankroll: f64, ev: f64, sd: f64, hours: usize, players: usize) -> Inputs {
        Inputs {
            hourly_ev: ev,
            hourly_sd: sd,
            starting_bankroll: bankroll,
            hours,
            players,
        }
    }

    #[test]
    fn reference_example_matches_unrounded_model() {
        let result = SimulationResult::simulate(input(10_000.0, 18.93, 330.97, 500, 1), 7).unwrap();
        let end = result.analytical.last().unwrap();
        assert!((end.mean - 19_465.0).abs() < 1e-9);
        assert!((end.one_above - 26_865.714185).abs() < 1e-5);
        assert!((end.one_below - 12_064.285815).abs() < 1e-5);
        assert!((end.two_above - 34_266.428_37).abs() < 1e-5);
        assert!((end.two_below - 4_663.571_63).abs() < 1e-5);
    }

    #[test]
    fn paths_stop_without_clamping_and_keep_hour_zero() {
        let result = SimulationResult::simulate(input(100.0, -60.0, 0.0, 4, 1), 1).unwrap();
        assert_eq!(result.paths[0], [100.0, 40.0, -20.0, -20.0, -20.0]);
        assert_eq!(result.depleted_hours, [Some(2)]);
        assert_eq!(
            result.analytical.iter().map(|p| p.mean).collect::<Vec<_>>(),
            [100.0, 40.0, -20.0, -80.0, -140.0]
        );
    }

    #[test]
    fn zero_sd_still_rounds_player_paths_each_hour() {
        let result = SimulationResult::simulate(input(100.0, 0.49, 0.0, 3, 1), 1).unwrap();
        assert_eq!(result.paths[0], [100.0, 100.0, 100.0, 100.0]);
        assert!((result.analytical[3].mean - 101.47).abs() < 1e-10);
    }

    #[test]
    fn negative_half_rounds_toward_zero_like_javascript() {
        let result = SimulationResult::simulate(input(1.0, -1.5, 0.0, 2, 1), 1).unwrap();
        assert_eq!(result.paths[0], [1.0, 0.0, 0.0]);
    }

    #[test]
    fn validation_rejects_invalid_and_accepts_edges() {
        assert!(input(-1.0, 0.0, 0.0, 1, 1).validate().is_err());
        assert!(
            input(0.0, -1_000_000.0, 1_000_000.0, 10_000, 9)
                .validate()
                .is_ok()
        );
        assert!(input(0.0, 0.0, 0.0, 0, 1).validate().is_err());
        assert!(input(0.0, 0.0, 0.0, 1, 10).validate().is_err());
        assert!(input(0.0, 0.29, 0.07, 1, 1).validate().is_ok());
        assert!(input(0.0, 1.234, 0.0, 1, 1).validate().is_err());
        assert!(input(0.0, f64::NAN, 0.0, 1, 1).validate().is_err());
    }

    #[test]
    fn zero_bankroll_stays_depleted_and_maximum_run_is_finite() {
        let zero = SimulationResult::simulate(input(0.0, 100.0, 100.0, 10, 1), 1).unwrap();
        assert!(zero.paths[0].iter().all(|&value| value == 0.0));
        assert_eq!(zero.depleted_hours, [Some(0)]);

        let maximum = SimulationResult::simulate(
            input(1_000_000_000.0, 1_000_000.0, 1_000_000.0, 10_000, 9),
            1,
        )
        .unwrap();
        assert_eq!(maximum.analytical.len(), 10_001);
        assert!(maximum.analytical.iter().all(|point| {
            [
                point.mean,
                point.one_above,
                point.one_below,
                point.two_above,
                point.two_below,
            ]
            .iter()
            .all(|value| value.is_finite())
        }));
    }

    #[test]
    fn same_seed_reproduces_and_players_are_independent() {
        let first = SimulationResult::simulate(input(10_000.0, 18.93, 330.97, 20, 3), 42).unwrap();
        let second = SimulationResult::simulate(input(10_000.0, 18.93, 330.97, 20, 3), 42).unwrap();
        assert_eq!(first.paths, second.paths);
        assert!(first.paths.windows(2).any(|paths| paths[0] != paths[1]));
    }
}
