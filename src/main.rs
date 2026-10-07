mod app;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180.0, 860.0])
            .with_min_inner_size([920.0, 700.0]),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };
    eframe::run_native(
        "21 Pro · Blackjack Strategy Trainer",
        options,
        Box::new(|cc| Ok(Box::new(app::TrainerApp::new(cc)))),
    )
}
