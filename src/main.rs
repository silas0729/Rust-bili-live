mod app;
mod backend;
mod bilibili;
mod config;
mod grpc;
mod live;

use anyhow::Result;
use eframe::egui::{ViewportBuilder, viewport::WindowLevel};

fn main() -> Result<()> {
    let initial_config = config::load();

    let viewport = ViewportBuilder::default()
        .with_title("Yuuna 弹幕")
        .with_inner_size([460.0, 720.0])
        .with_min_inner_size([420.0, 560.0])
        .with_resizable(true)
        .with_transparent(true)
        .with_decorations(false)
        .with_window_level(if initial_config.always_on_top {
            WindowLevel::AlwaysOnTop
        } else {
            WindowLevel::Normal
        });

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Yuuna 弹幕",
        options,
        Box::new(move |cc| {
            Ok(Box::new(
                app::YuunaApp::new(cc, initial_config.clone()).map_err(|err| err.to_string())?,
            ))
        }),
    )
    .map_err(|err| anyhow::anyhow!(err.to_string()))
}
