use super::{Command, Feedback};
use eframe::egui::{
    self, Align2, Color32, FontId, RichText, Sense, Stroke, StrokeKind, Vec2, pos2, vec2,
};
use twenty_one_pro::{
    game::{Game, Phase},
    model::{Card, HandKind, hand_value},
    training::{Analytics, CellStats},
};

pub const BG: Color32 = Color32::from_rgb(15, 23, 32);
pub const SURFACE: Color32 = Color32::from_rgb(23, 34, 45);
pub const BORDER: Color32 = Color32::from_rgb(45, 61, 73);
pub const TEXT: Color32 = Color32::from_rgb(235, 235, 225);
pub const MUTED: Color32 = Color32::from_rgb(151, 171, 184);
pub const GREEN: Color32 = Color32::from_rgb(112, 208, 179);
pub const GOLD: Color32 = Color32::from_rgb(231, 184, 111);
pub const RED: Color32 = Color32::from_rgb(231, 126, 115);
const FELT: Color32 = Color32::from_rgb(20, 48, 45);

pub fn configure(ctx: &egui::Context) {
    let mut style = egui::Style {
        visuals: egui::Visuals::dark(),
        ..Default::default()
    };
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = SURFACE;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.extreme_bg_color = BG;
    style.visuals.selection.bg_fill = Color32::from_rgb(42, 85, 77);
    style.visuals.selection.stroke = Stroke::new(1.0, GREEN);
    style.visuals.widgets.inactive.bg_fill = SURFACE;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(39, 64, 68);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(39, 64, 68);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, GREEN);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(42, 85, 77);
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 10.0);
    style
        .text_styles
        .insert(egui::TextStyle::Body, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, FontId::proportional(15.0));
    style
        .text_styles
        .insert(egui::TextStyle::Small, FontId::proportional(12.0));
    style
        .text_styles
        .insert(egui::TextStyle::Heading, FontId::proportional(25.0));
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn panel() -> egui::Frame {
    egui::Frame::NONE
        .fill(SURFACE)
        .corner_radius(12)
        .inner_margin(18)
        .stroke(Stroke::new(1.0, BORDER))
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(11.0).color(GREEN).strong());
}
pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(RichText::new(text.into()).color(MUTED));
}
pub fn heading(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    eyebrow(ui, "21 PRO  /  STRATEGY LAB");
    ui.label(RichText::new(title).size(32.0).strong());
    muted(ui, subtitle);
    ui.add_space(12.0);
}
pub fn primary(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).color(BG).strong())
            .fill(GREEN)
            .corner_radius(8)
            .min_size(vec2(140.0, 40.0)),
    )
}
pub fn metric(ui: &mut egui::Ui, label: &str, value: &str, detail: &str) {
    panel().inner_margin(12).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 4.0;
        ui.set_min_width((ui.available_width() - 1.0).max(0.0));
        ui.label(RichText::new(label).size(12.0).color(MUTED));
        ui.label(RichText::new(value).size(26.0).strong());
        ui.label(RichText::new(detail).size(11.0).color(MUTED));
    });
}
pub fn accuracy(stats: CellStats) -> String {
    stats
        .accuracy()
        .map(|value| format!("{:.0}%", value * 100.0))
        .unwrap_or_else(|| "—".to_owned())
}
pub fn due_label(due: Option<i64>, now: i64) -> String {
    match due {
        None => "No review scheduled".to_owned(),
        Some(due) if due <= now => "Due now".to_owned(),
        Some(due) => {
            let minutes = (due - now + 59) / 60;
            if minutes < 60 {
                format!("Due in {minutes}m")
            } else if minutes < 1440 {
                format!("Due in {}h", (minutes + 59) / 60)
            } else {
                format!("Due in {}d", (minutes + 1439) / 1440)
            }
        }
    }
}
pub fn units(half_units: i32) -> String {
    format!("{:+.1} units", f64::from(half_units) / 2.0)
}

fn paint_card(ui: &egui::Ui, rect: egui::Rect, card: Option<Card>) {
    let painter = ui.painter();
    if let Some(card) = card {
        painter.rect_filled(rect, 6, Color32::from_rgb(244, 240, 225));
        painter.rect_stroke(
            rect,
            6,
            Stroke::new(1.0, Color32::from_rgb(204, 211, 203)),
            StrokeKind::Inside,
        );
        let ink = if card.suit.is_red() {
            Color32::from_rgb(174, 62, 57)
        } else {
            Color32::from_rgb(24, 40, 49)
        };
        painter.text(
            rect.left_top() + vec2(7.0, 7.0),
            Align2::LEFT_TOP,
            card.rank.label(),
            FontId::proportional(21.0),
            ink,
        );
        painter.text(
            rect.center() + vec2(1.0, 6.0),
            Align2::CENTER_CENTER,
            card.suit.symbol(),
            FontId::proportional(31.0),
            ink,
        );
    } else {
        painter.rect_filled(rect, 6, Color32::from_rgb(35, 70, 69));
        painter.rect_stroke(
            rect,
            6,
            Stroke::new(1.5, Color32::from_rgb(97, 140, 126)),
            StrokeKind::Inside,
        );
        painter.rect_stroke(
            rect.shrink(6.0),
            3,
            Stroke::new(1.0, Color32::from_rgb(68, 104, 94)),
            StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "21",
            FontId::proportional(25.0),
            GOLD,
        );
    }
}
fn cards(ui: &mut egui::Ui, cards: &[Card], hide_hole: bool) {
    let count = cards.len().max(2);
    let card_size = vec2(52.0, 74.0);
    let step = ((ui.available_width() - card_size.x) / (count - 1) as f32).clamp(12.0, 60.0);
    let width = card_size.x + step * (count - 1) as f32;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), card_size.y), Sense::hover());
    response.widget_info(|| {
        use std::fmt::Write;
        let mut label = String::from("Cards: ");
        for index in 0..count {
            if index > 0 {
                label.push_str(", ");
            }
            if let Some(card) = cards.get(index).filter(|_| !hide_hole || index == 0) {
                let _ = write!(label, "{:?} of {:?}", card.rank, card.suit);
            } else {
                label.push_str("face-down card");
            }
        }
        egui::WidgetInfo::labeled(egui::WidgetType::Image, true, label)
    });
    let start = pos2(rect.center().x - width / 2.0, rect.top());
    for index in 0..count {
        let card = if hide_hole && index > 0 {
            None
        } else {
            cards.get(index).copied()
        };
        paint_card(
            ui,
            egui::Rect::from_min_size(start + vec2(step * index as f32, 0.0), card_size),
            card,
        );
    }
}

pub fn game_table(ui: &mut egui::Ui, game: &Game) {
    egui::Frame::NONE
        .fill(FELT)
        .corner_radius(16)
        .inner_margin(14)
        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 82, 72)))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("DEALER").size(11.0).color(GREEN).strong());
                cards(ui, &game.dealer, game.phase != Phase::Finished);
                if game.phase == Phase::Finished {
                    let value = hand_value(&game.dealer);
                    ui.label(
                        RichText::new(format!(
                            "{}{}",
                            if value.soft { "Soft " } else { "" },
                            value.total
                        ))
                        .color(MUTED),
                    );
                } else {
                    ui.label(
                        RichText::new("Hits soft 17 · hole card stays hidden")
                            .size(12.0)
                            .color(MUTED),
                    );
                }
            });
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);
            if game.hands.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("YOUR HAND").size(11.0).color(GREEN).strong());
                    cards(ui, &[], false);
                    muted(ui, "Train the decision. Not the outcome.");
                });
            } else {
                let columns = game.hands.len().min(2);
                for (row, chunk) in game.hands.chunks(2).enumerate() {
                    ui.columns(columns, |uis| {
                        for (column, hand) in chunk.iter().enumerate() {
                            let index = row * 2 + column;
                            let ui = &mut uis[column];
                            let active = game.phase != Phase::Finished && game.active == index;
                            egui::Frame::NONE
                                .inner_margin(8)
                                .corner_radius(8)
                                .stroke(Stroke::new(
                                    1.0,
                                    if active { GREEN } else { Color32::TRANSPARENT },
                                ))
                                .show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        let label = if game.hands.len() == 1 {
                                            "YOUR HAND".to_owned()
                                        } else {
                                            format!("HAND {}", index + 1)
                                        };
                                        ui.label(
                                            RichText::new(label)
                                                .size(11.0)
                                                .color(if active { GREEN } else { MUTED })
                                                .strong(),
                                        );
                                        cards(ui, &hand.cards, false);
                                        let value = hand_value(&hand.cards);
                                        ui.label(
                                            RichText::new(format!(
                                                "{}{}",
                                                if value.soft { "Soft " } else { "" },
                                                value.total
                                            ))
                                            .size(20.0)
                                            .strong(),
                                        );
                                        if let Some(result) = &game.result {
                                            if let Some(outcome) = result.outcomes.get(index) {
                                                ui.label(
                                                    RichText::new(&outcome.label).color(MUTED),
                                                );
                                            }
                                        } else if hand.split_aces {
                                            muted(ui, "Split aces · one card each");
                                        } else if hand.from_split {
                                            muted(ui, "Split hand · no surrender");
                                        }
                                    });
                                });
                        }
                    });
                    if row == 0 && game.hands.len() > 2 {
                        ui.add_space(8.0);
                    }
                }
            }
        });
}

pub fn actions(ui: &mut egui::Ui, game: &Game, command: &mut Option<Command>) {
    if let Some(situation) = game.situation() {
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            for action in twenty_one_pro::model::Action::ALL {
                let insurance = matches!(
                    action,
                    twenty_one_pro::model::Action::Insure
                        | twenty_one_pro::model::Action::DeclineInsurance
                );
                if insurance != (situation.kind == HandKind::Insurance) {
                    continue;
                }
                let label = format!("{}  [{}]", action.label(), action.short());
                let button = egui::Button::new(RichText::new(label).strong())
                    .min_size(vec2(104.0, 42.0))
                    .corner_radius(8);
                if ui.add_enabled(situation.allows(action), button).clicked() {
                    *command = Some(Command::Act(action));
                }
            }
        });
        ui.label(RichText::new(situation.context()).size(12.0).color(MUTED));
        if game.phase == Phase::Insurance {
            muted(
                ui,
                "A separate half-unit wager. Choose before playing your hand.",
            );
        }
    }
}

pub fn feedback(ui: &mut egui::Ui, records: &[Feedback]) {
    let Some(last) = records.last() else {
        return;
    };
    ui.add_space(12.0);
    let color = if last.correct() { GREEN } else { GOLD };
    panel().inner_margin(12).stroke(Stroke::new(1.0, color)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(if last.correct() { "Correct decision" } else { "Strategy correction" }).color(color).strong());
        ui.label(format!("{}  ·  Played: {}  ·  Best: {}", last.situation.label(), last.chosen.label(), last.expected.label()));
        muted(ui, last.explanation);
        if !last.correct() { ui.label(RichText::new("Your move was applied. The result of the hand does not change this assessment.").size(12.0).color(MUTED)); }
        if records.len() > 1 {
            egui::CollapsingHeader::new("Earlier decisions this round").show(ui, |ui| {
                for record in &records[..records.len() - 1] {
                    ui.colored_label(if record.correct() { GREEN } else { GOLD }, format!("{}: {} · best {}", record.situation.label(), record.chosen.label(), record.expected.label()));
                }
            });
        }
    });
}

pub fn heatmap(
    ui: &mut egui::Ui,
    analytics: &Analytics,
    kind: HandKind,
) -> Option<(HandKind, u8, u8)> {
    let values = match kind {
        HandKind::Hard => 4..=20,
        HandKind::Soft => 13..=20,
        HandKind::Pair => 2..=11,
        HandKind::Insurance => 0..=0,
    };
    let cell_width = ((ui.available_width() - 104.0) / 10.0).clamp(28.0, 62.0);
    let mut selected = None;
    egui::Grid::new("decision-heatmap")
        .spacing(vec2(4.0, 4.0))
        .show(ui, |ui| {
            ui.label(RichText::new("YOU / DEALER").size(10.0).color(MUTED));
            for dealer in 2..=11 {
                let label = if dealer == 11 {
                    "A".to_owned()
                } else {
                    dealer.to_string()
                };
                let (rect, _) = ui.allocate_exact_size(vec2(cell_width, 22.0), Sense::hover());
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(13.0),
                    MUTED,
                );
            }
            ui.end_row();
            for value in values {
                let label = match kind {
                    HandKind::Pair if value == 11 => "A,A".to_owned(),
                    HandKind::Pair => format!("{value},{value}"),
                    HandKind::Hard => format!("Hard {value}"),
                    HandKind::Soft => format!("Soft {value}"),
                    HandKind::Insurance => "Insurance".to_owned(),
                };
                ui.label(RichText::new(label).size(12.0));
                for dealer in 2..=11 {
                    let stats = analytics
                        .cells
                        .get(&(kind, value, dealer))
                        .copied()
                        .unwrap_or_default();
                    let rate = if stats.attempts == 0 {
                        0.0
                    } else {
                        stats.mistakes as f32 / stats.attempts as f32
                    };
                    let fill = if stats.attempts == 0 {
                        BG
                    } else {
                        blend(
                            Color32::from_rgb(38, 97, 82),
                            Color32::from_rgb(177, 77, 64),
                            rate,
                        )
                    };
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(cell_width, 27.0), Sense::click());
                    ui.painter().rect_filled(rect, 4, fill);
                    let label = if stats.attempts == 0 {
                        "—".to_owned()
                    } else {
                        format!("{:.0}%", rate * 100.0)
                    };
                    ui.painter().text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        label,
                        FontId::proportional(11.0),
                        if stats.attempts == 0 { MUTED } else { TEXT },
                    );
                    let detail = if stats.attempts == 0 {
                        "No observations. This is not evidence of mastery.".to_owned()
                    } else {
                        format!(
                            "{} mistakes / {} decisions\nClick to inspect the exact rule contexts.",
                            stats.mistakes, stats.attempts
                        )
                    };
                    if response.on_hover_text(detail).clicked() {
                        selected = Some((kind, value, dealer));
                    }
                }
                ui.end_row();
            }
        });
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(MUTED, "—  Unseen");
        ui.colored_label(GREEN, "0%  No recorded mistakes");
        ui.colored_label(RED, "100%  Every answer missed");
    });
    selected
}
fn blend(a: Color32, b: Color32, amount: f32) -> Color32 {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * amount).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

pub fn trend(ui: &mut egui::Ui, analytics: &Analytics) {
    ui.label(RichText::new("Accuracy over time").strong());
    if analytics.trend.is_empty() {
        muted(ui, "Play a decision to begin your trend.");
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 170.0), Sense::hover());
    let plot = egui::Rect::from_min_max(rect.min + vec2(40.0, 12.0), rect.max - vec2(14.0, 28.0));
    let painter = ui.painter();
    for (value, label) in [(0.0, "0%"), (0.5, "50%"), (1.0, "100%")] {
        let y = plot.bottom() - plot.height() * value;
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0, BORDER),
        );
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            label,
            FontId::proportional(10.0),
            MUTED,
        );
    }
    let denominator = (analytics.trend.len().saturating_sub(1)).max(1) as f32;
    let mut previous = None;
    for (index, point) in analytics.trend.iter().enumerate() {
        let position = pos2(
            plot.left() + index as f32 / denominator * plot.width(),
            plot.bottom() - point.accuracy * plot.height(),
        );
        if let Some(previous) = previous {
            painter.line_segment([previous, position], Stroke::new(2.0, GREEN));
        }
        painter.circle_filled(
            position,
            3.0,
            if point.attempts < 25 { GOLD } else { GREEN },
        );
        let response = ui.interact(
            egui::Rect::from_center_size(position, Vec2::splat(16.0)),
            ui.id().with(("trend", index)),
            Sense::hover(),
        );
        response.on_hover_text(format!(
            "Block {}: {:.0}% correct across {} decisions",
            point.index,
            point.accuracy * 100.0,
            point.attempts
        ));
        previous = Some(position);
    }
    painter.text(
        pos2(plot.center().x, rect.bottom() - 8.0),
        Align2::CENTER_BOTTOM,
        "Consecutive 25-decision blocks · gold = partial block",
        FontId::proportional(11.0),
        MUTED,
    );
}

pub fn category_bars(ui: &mut egui::Ui, analytics: &Analytics) {
    ui.label(RichText::new("Mistakes by hand family").strong());
    for kind in [
        HandKind::Hard,
        HandKind::Soft,
        HandKind::Pair,
        HandKind::Insurance,
    ] {
        let stats = analytics.categories.get(&kind).copied().unwrap_or_default();
        let rate = if stats.attempts == 0 {
            0.0
        } else {
            stats.mistakes as f32 / stats.attempts as f32
        };
        ui.label(
            RichText::new(format!(
                "{}  ·  {} / {} missed",
                kind.label(),
                stats.mistakes,
                stats.attempts
            ))
            .size(12.0)
            .color(MUTED),
        );
        ui.add(egui::ProgressBar::new(rate).fill(GOLD).desired_height(7.0));
    }
}
