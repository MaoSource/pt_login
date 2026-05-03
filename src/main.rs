mod app;
mod jsonp;
mod login_service;
mod qq_util;
mod types;

use anyhow::Context;
use app::DaojuLoginApp;
use tracing_subscriber::EnvFilter;

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

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("道聚城扫码登录")
            .with_inner_size([900.0, 560.0])
            .with_min_inner_size([700.0, 460.0]),
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
