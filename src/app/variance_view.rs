use super::{Command, TrainerApp, VarianceState, widgets as w};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, TextEdit, vec2,
};
use twenty_one_pro::variance::{AnalyticalPoint, Inputs, SimulationResult};

impl TrainerApp {
    pub(super) fn variance_view(&mut self, ui: &mut egui::Ui, command: &mut Option<Command>) {
        w::heading(
            ui,
            "Variance Visualizer",
            "See how identical EV and volatility can produce different bankroll histories.",
        );
        w::panel().show(ui, |ui| {
            ui.label("Explore an hourly bankroll model with editable assumptions. Each run samples fresh possible histories; it is not a prediction.");
            w::muted(ui, "Example inputs only. Enter hourly EV and standard deviation for the game and betting strategy you want to explore. These values are not calculated from your training results.");
            ui.add_space(10.0);
            controls(ui, &mut self.variance, command);
            ui.add_space(8.0);
            if w::primary(ui, "Run simulation").clicked() {
                *command = Some(Command::RunVariance);
            }
        });
        ui.add_space(14.0);
        if let Some(result) = &self.variance.result {
            let stale = !same_inputs(&self.variance, result.inputs);
            if stale {
                ui.colored_label(w::GOLD, "Inputs changed — run again");
            }
            if stale {
                w::muted(
                    ui,
                    "The chart and all values below still describe the previous successful run.",
                );
            }
            render_result(ui, result, self.variance.show_sd);
        } else {
            w::panel().show(ui, |ui| {
                ui.label(egui::RichText::new("No simulation yet").size(20.0).strong());
                w::muted(ui, "Edit the example values if you like, then press Run simulation to draw possible paths.");
            });
        }
    }
}

fn controls(ui: &mut egui::Ui, state: &mut VarianceState, _command: &mut Option<Command>) {
    ui.horizontal_wrapped(|ui| {
        field(
            ui,
            "Hourly EV ($/hour)",
            &mut state.hourly_ev,
            state.errors[0].as_deref(),
        );
        field(
            ui,
            "Hourly SD ($/hour)",
            &mut state.hourly_sd,
            state.errors[1].as_deref(),
        );
        field(
            ui,
            "Starting bankroll ($)",
            &mut state.bankroll,
            state.errors[2].as_deref(),
        );
        field(ui, "Hours", &mut state.hours, state.errors[3].as_deref());
        ui.vertical(|ui| {
            egui::ComboBox::from_label("Players simulated")
                .selected_text(state.players.to_string())
                .show_ui(ui, |ui| {
                    for players in 1..=9 {
                        ui.selectable_value(&mut state.players, players, players.to_string());
                    }
                });
        });
        ui.vertical(|ui| {
            ui.label("Chart overlays");
            ui.checkbox(&mut state.show_sd, "Show SD lines");
        });
    });
}

fn field(ui: &mut egui::Ui, label: &str, value: &mut String, error: Option<&str>) {
    ui.vertical(|ui| {
        let label_response = ui.label(label);
        ui.add(
            TextEdit::singleline(value)
                .desired_width(132.0)
                .vertical_align(egui::Align::Center),
        )
        .labelled_by(label_response.id);
        if let Some(error) = error {
            ui.colored_label(w::RED, error);
        }
    });
}

fn same_inputs(state: &VarianceState, expected: Inputs) -> bool {
    let mut errors = [None, None, None, None];
    let Some(hourly_ev) = super::parse_money(&state.hourly_ev, &mut errors[0], "Hourly EV") else {
        return false;
    };
    let Some(hourly_sd) = super::parse_money(&state.hourly_sd, &mut errors[1], "Hourly SD") else {
        return false;
    };
    let Some(starting_bankroll) = super::parse_money(&state.bankroll, &mut errors[2], "Bankroll")
    else {
        return false;
    };
    let Some(hours) = super::parse_integer(&state.hours, &mut errors[3], "Hours") else {
        return false;
    };
    Inputs {
        hourly_ev,
        hourly_sd,
        starting_bankroll,
        hours,
        players: state.players,
    } == expected
}

fn render_result(ui: &mut egui::Ui, result: &SimulationResult, show_sd: bool) {
    summary(ui, result);
    ui.add_space(12.0);
    w::panel().show(ui, |ui| chart(ui, result, show_sd));
    ui.add_space(12.0);
    w::panel().show(ui, explanation);
}

fn summary(ui: &mut egui::Ui, result: &SimulationResult) {
    let end = result.analytical.last().expect("simulation has hour zero");
    ui.label(
        egui::RichText::new("Displayed result snapshot")
            .size(20.0)
            .strong(),
    );
    w::muted(
        ui,
        format!(
            "EV ${:.2}/hour · SD ${:.2}/hour · starting bankroll ${:.2} · {} hours · {} player{} · seed {}",
            result.inputs.hourly_ev,
            result.inputs.hourly_sd,
            result.inputs.starting_bankroll,
            result.inputs.hours,
            result.inputs.players,
            if result.inputs.players == 1 { "" } else { "s" },
            result.seed
        ),
    );
    ui.label(format!(
        "Unstopped EV reference at hour {}: {}",
        result.inputs.hours,
        money(end.mean)
    ));
    ui.label(format!(
        "Unstopped model ranges at hour {}: ±1 SD {} to {}; ±2 SD {} to {}",
        result.inputs.hours,
        money(end.one_below),
        money(end.one_above),
        money(end.two_below),
        money(end.two_above)
    ));
    w::muted(
        ui,
        "Reference and ranges are the unstopped, unrounded normal model before hourly player-path rounding and depletion stopping; they are not expected stopped outcomes.",
    );
    ui.add_space(6.0);
    for (index, path) in result.paths.iter().enumerate() {
        let ending = *path.last().unwrap_or(&result.inputs.starting_bankroll);
        let depleted = result.depleted_hours[index].map_or_else(
            || "not depleted".to_owned(),
            |hour| format!("depleted at hour {hour}"),
        );
        ui.label(format!(
            "Player {}: {} ending bankroll · {} net profit · {}",
            index + 1,
            money(ending),
            money(ending - result.inputs.starting_bankroll),
            depleted
        ));
    }
}

fn chart(ui: &mut egui::Ui, result: &SimulationResult, show_sd: bool) {
    ui.label(egui::RichText::new("Bankroll history").size(20.0).strong());
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 410.0), Sense::hover());
    let plot = Rect::from_min_max(rect.min + vec2(66.0, 18.0), rect.max - vec2(18.0, 42.0));
    let mut min = 0.0_f64;
    let mut max = 0.0_f64;
    for point in &result.analytical {
        for value in [
            point.mean,
            point.one_above,
            point.one_below,
            point.two_above,
            point.two_below,
        ] {
            if show_sd || value == point.mean {
                min = min.min(value);
                max = max.max(value);
            }
        }
    }
    for path in &result.paths {
        for &value in path {
            min = min.min(value);
            max = max.max(value);
        }
    }
    let padding = ((max - min).abs() * 0.08).max(1.0);
    min -= padding;
    max += padding;
    let span = (max - min).max(1.0);
    let x = |hour: usize| plot.left() + hour as f32 / result.inputs.hours as f32 * plot.width();
    let y = |value: f64| plot.bottom() - ((value - min) / span) as f32 * plot.height();
    let painter = ui.painter();
    painter.rect_stroke(plot, 0.0, Stroke::new(1.0, w::BORDER), StrokeKind::Inside);
    for fraction in [0.0, 0.5, 1.0] {
        let value = min + span * fraction;
        let position = y(value);
        painter.line_segment(
            [
                Pos2::new(plot.left(), position),
                Pos2::new(plot.right(), position),
            ],
            Stroke::new(1.0, w::BORDER),
        );
        painter.text(
            Pos2::new(plot.left() - 8.0, position),
            Align2::RIGHT_CENTER,
            money(value),
            FontId::proportional(10.0),
            w::MUTED,
        );
    }
    for fraction in [0.0, 0.5, 1.0] {
        let hour = (result.inputs.hours as f32 * fraction).round() as usize;
        painter.text(
            Pos2::new(x(hour), plot.bottom() + 8.0),
            Align2::CENTER_TOP,
            hour.to_string(),
            FontId::proportional(10.0),
            w::MUTED,
        );
    }
    painter.text(
        Pos2::new(plot.center().x, rect.bottom() - 8.0),
        Align2::CENTER_BOTTOM,
        "Hours",
        FontId::proportional(11.0),
        w::TEXT,
    );
    painter.text(
        Pos2::new(rect.left() + 8.0, plot.center().y),
        Align2::CENTER_CENTER,
        "Bankroll ($)",
        FontId::proportional(11.0),
        w::TEXT,
    );
    draw_series(
        painter,
        result,
        |point| point.mean,
        w::GREEN,
        Stroke::new(2.5, w::GREEN),
        x,
        y,
    );
    if show_sd {
        draw_series(
            painter,
            result,
            |point| point.one_above,
            w::GOLD,
            Stroke::new(1.0, w::GOLD),
            x,
            y,
        );
        draw_series(
            painter,
            result,
            |point| point.one_below,
            w::GOLD,
            Stroke::new(1.0, w::GOLD),
            x,
            y,
        );
        draw_series(
            painter,
            result,
            |point| point.two_above,
            w::MUTED,
            Stroke::new(1.0, w::MUTED),
            x,
            y,
        );
        draw_series(
            painter,
            result,
            |point| point.two_below,
            w::MUTED,
            Stroke::new(1.0, w::MUTED),
            x,
            y,
        );
    }
    for (index, path) in result.paths.iter().enumerate() {
        let color = player_color(index);
        let stroke = Stroke::new(1.4, color);
        for (hour, pair) in path.windows(2).enumerate() {
            painter.line_segment(
                [
                    Pos2::new(x(hour), y(pair[0])),
                    Pos2::new(x(hour + 1), y(pair[1])),
                ],
                stroke,
            );
        }
    }
    legend(ui, show_sd, result.inputs.players);
    if let Some(pointer) = response
        .hover_pos()
        .filter(|position| plot.contains(*position))
    {
        let hour = (((pointer.x - plot.left()) / plot.width()) * result.inputs.hours as f32)
            .round()
            .clamp(0.0, result.inputs.hours as f32) as usize;
        let point = &result.analytical[hour];
        let mut text = format!("Hour {hour}\nMean: {}", money(point.mean));
        if show_sd {
            text.push_str(&format!(
                "\n+1 SD: {}\n-1 SD: {}\n+2 SD: {}\n-2 SD: {}",
                money(point.one_above),
                money(point.one_below),
                money(point.two_above),
                money(point.two_below)
            ));
        }
        for (index, path) in result.paths.iter().enumerate() {
            text.push_str(&format!("\nPlayer {}: {}", index + 1, money(path[hour])));
        }
        response.clone().on_hover_text(text);
    }
}

fn draw_series<F>(
    painter: &egui::Painter,
    result: &SimulationResult,
    value: F,
    color: Color32,
    stroke: Stroke,
    x: impl Fn(usize) -> f32,
    y: impl Fn(f64) -> f32,
) where
    F: Fn(&AnalyticalPoint) -> f64,
{
    for (hour, points) in result.analytical.windows(2).enumerate() {
        painter.line_segment(
            [
                Pos2::new(x(hour), y(value(&points[0]))),
                Pos2::new(x(hour + 1), y(value(&points[1]))),
            ],
            stroke,
        );
    }
    painter.text(
        Pos2::new(
            x(result.inputs.hours),
            y(value(result.analytical.last().unwrap())),
        ),
        Align2::LEFT_CENTER,
        match color {
            c if c == w::GREEN => "EV mean",
            c if c == w::GOLD => "±1 SD",
            _ => "±2 SD",
        },
        FontId::proportional(10.0),
        color,
    );
}

fn player_color(index: usize) -> Color32 {
    [
        Color32::from_rgb(111, 173, 224),
        Color32::from_rgb(209, 127, 207),
        Color32::from_rgb(226, 143, 87),
        Color32::from_rgb(161, 214, 112),
        Color32::from_rgb(230, 152, 194),
        Color32::from_rgb(122, 207, 201),
        Color32::from_rgb(236, 196, 82),
        Color32::from_rgb(164, 143, 224),
        Color32::from_rgb(222, 111, 111),
    ][index]
}

fn legend(ui: &mut egui::Ui, show_sd: bool, players: usize) {
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(w::GREEN, "━━ EV mean");
        if show_sd {
            ui.colored_label(w::GOLD, "─ ±1 SD");
            ui.colored_label(w::MUTED, "┄ ±2 SD");
        }
        for index in 0..players {
            ui.colored_label(player_color(index), format!("Player {}", index + 1));
        }
    });
}
fn explanation(ui: &mut egui::Ui) {
    ui.label(
        egui::RichText::new("How to read this model")
            .size(20.0)
            .strong(),
    );
    ui.label("EV grows linearly with time; the standard deviation of accumulated results grows with the square root of time. At a fixed hour, about 68% and 95% of the unstopped, unrounded normal model lie inside ±1 and ±2 SD. These are not probabilities of staying inside the bands for the whole run, and depletion stopping changes the distribution.");
    ui.label("Player gains and losses are rounded to whole dollars each hour, while the reference lines are not. For example, EV $0.49/hour with SD $0 leaves a player path unchanged even as the EV reference rises. Being below EV does not make future gains due.");
    ui.label("Players show different possible histories; they do not change the odds or estimate risk of ruin reliably from at most nine paths. Longer play can reduce relative deviation while absolute dollar deviations still grow. The model assumes constant EV/SD, does not deal cards or resize bets, replenish bankroll, or model within-hour ruin, fatigue, expenses, changing conditions, or mistakes. Positive EV is not a profit guarantee.");
}

fn money(value: f64) -> String {
    format!("${value:.2}")
}
