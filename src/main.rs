#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod jsonp;
mod login_service;
mod qq_util;
mod types;
mod user_service;

use anyhow::Context;
use app::DaojuLoginApp;
use eframe::egui;
use tracing_subscriber::EnvFilter;

fn load_app_icon() -> anyhow::Result<egui::IconData> {
    let bytes = std::fs::read("assets/icon.png").context("failed to read assets/icon.png")?;
    let image = image::load_from_memory(&bytes)
        .context("failed to decode assets/icon.png")?
        .into_rgba8();
    let (width, height) = image.dimensions();

    Ok(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("daoju-login-runtime")
        .build()
        .context("failed to create tokio runtime")?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("道聚城扫码登录")
        .with_inner_size([860.0, 600.0])
        .with_min_inner_size([720.0, 500.0]);

    match load_app_icon() {
        Ok(icon) => {
            viewport = viewport.with_icon(icon);
        }
        Err(err) => {
            tracing::warn!("{err:#}");
        }
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let mut runtime = Some(runtime);
    eframe::run_native(
        "道聚城扫码登录",
        options,
        Box::new(move |cc| {
            Ok(Box::new(DaojuLoginApp::new(
                cc,
                runtime.take().expect("app creator called more than once"),
            )))
        }),
    )
    .map_err(|err| anyhow::anyhow!("eframe error: {err}"))
}
