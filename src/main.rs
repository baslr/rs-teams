#![allow(dead_code)]

mod api;
mod app;
mod auth;
mod error;
mod gui;
mod models;

fn main() -> iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "teams_rs=info".parse().unwrap()),
        )
        .init();

    iced::application(app::boot, app::update, app::view)
        .title("Teams")
        .theme(app::theme)
        .subscription(app::subscription)
        .font(include_bytes!("../fonts/FiraSans-Regular.ttf").as_slice())
        .font(include_bytes!("../fonts/FiraSans-Bold-Renamed.ttf").as_slice())
        .window_size((1200.0, 800.0))
        .run()
}
