use crate::login_service;
use crate::types::{LoginEvent, LoginResult, LoginStatus, QrCodeInfo};
use eframe::egui;
use egui::{
    Align, Color32, ColorImage, CornerRadius, FontData, FontDefinitions, FontFamily, FontId,
    Layout, Margin, RichText, Sense, Stroke, TextureHandle, Vec2,
};
use std::fs;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};
use tokio::task::JoinHandle;

const QR_TTL: Duration = Duration::from_secs(120);

// Design tokens — mirror `colors_and_type.css` from the Claude Design bundle
// so the egui surface stays in lockstep with the source mockup.
mod tok {
    use eframe::egui::Color32;
    pub const BG: Color32 = Color32::from_rgb(255, 255, 255);
    pub const BG_SOFT: Color32 = Color32::from_rgb(247, 247, 248);
    pub const BG_MUTE: Color32 = Color32::from_rgb(236, 236, 236);
    pub const BG_HOVER: Color32 = Color32::from_rgb(224, 224, 224);
    pub const TEXT: Color32 = Color32::from_rgb(13, 13, 13);
    pub const TEXT_SOFT: Color32 = Color32::from_rgb(64, 65, 79);
    pub const TEXT_MUTE: Color32 = Color32::from_rgb(110, 110, 128);
    pub const TEXT_FAINT: Color32 = Color32::from_rgb(180, 180, 194);
    pub const BORDER: Color32 = Color32::from_rgb(229, 229, 229);
    pub const BORDER_SOFT: Color32 = Color32::from_rgb(236, 236, 236);
    pub const ACCENT: Color32 = Color32::from_rgb(16, 163, 127);
    pub const INFO: Color32 = Color32::from_rgb(59, 130, 246);
    pub const WARN: Color32 = Color32::from_rgb(217, 119, 6);
    pub const DANGER: Color32 = Color32::from_rgb(239, 68, 68);
    pub const NEUTRAL: Color32 = Color32::from_rgb(142, 142, 160);
    pub const CODE_BG: Color32 = Color32::from_rgb(31, 35, 41);
    pub const CODE_FG: Color32 = Color32::from_rgb(230, 230, 230);
    pub const CODE_TIME: Color32 = Color32::from_rgb(86, 88, 105);
    pub const CODE_SUCCESS: Color32 = Color32::from_rgb(25, 195, 125);
}

pub struct DaojuLoginApp {
    runtime: tokio::runtime::Runtime,
    rx: Option<UnboundedReceiver<LoginEvent>>,
    task: Option<JoinHandle<()>>,
    generation: u64,
    active_generation: u64,
    status: LoginStatus,
    status_detail: String,
    qr_texture: Option<TextureHandle>,
    qr_loaded_at: Option<Instant>,
    qr_base64_len: usize,
    result: Option<LoginResult>,
    logs: Vec<LogEntry>,
    copied_logs_until: Option<Instant>,
    auto_started: bool,
}

#[derive(Debug, Clone)]
struct LogEntry {
    time: String,
    line: String,
    kind: LogKind,
}

#[derive(Debug, Clone, Copy)]
enum LogKind {
    Info,
    Success,
    Warn,
    Error,
    Muted,
}

impl DaojuLoginApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: tokio::runtime::Runtime) -> Self {
        install_chinese_font(&cc.egui_ctx);
        install_theme(&cc.egui_ctx);

        Self {
            runtime,
            rx: None,
            task: None,
            generation: 0,
            active_generation: 0,
            status: LoginStatus::Idle,
            status_detail: String::new(),
            qr_texture: None,
            qr_loaded_at: None,
            qr_base64_len: 0,
            result: None,
            logs: Vec::new(),
            copied_logs_until: None,
            auto_started: false,
        }
    }

    fn start_login(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }

        self.generation = self.generation.wrapping_add(1);
        self.active_generation = self.generation;
        let generation = self.generation;
        let (tx, rx) = unbounded_channel();
        let wrapped_tx = tx.clone();

        self.rx = Some(rx);
        self.status = LoginStatus::RequestingQrCode;
        self.status_detail.clear();
        self.result = None;
        self.qr_texture = None;
        self.qr_loaded_at = None;
        self.qr_base64_len = 0;
        self.append_log_kind("开始获取登录二维码。", LogKind::Info);

        self.task = Some(self.runtime.spawn(async move {
            let (inner_tx, mut inner_rx) = unbounded_channel();
            tokio::spawn(login_service::run_login_flow(inner_tx));

            while let Some(event) = inner_rx.recv().await {
                let _ = wrapped_tx.send(event);
            }
            let _ = wrapped_tx.send(LoginEvent::Finished);
            let _ = generation;
        }));
    }

    fn drain_events(&mut self, ctx: &egui::Context) {
        let Some(rx) = &mut self.rx else {
            return;
        };

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        for event in events {
            match event {
                LoginEvent::Status(status) => {
                    self.append_status_log(&status);
                    self.status = status;
                    self.status_detail.clear();
                }
                LoginEvent::QrCode(qr) => {
                    self.load_qr_texture(ctx, qr);
                    self.append_log_kind(
                        format!("二维码已生成，图片 base64 长度 {}。", self.qr_base64_len),
                        LogKind::Success,
                    );
                    self.append_log_kind("请使用 QQ 扫码。", LogKind::Info);
                }
                LoginEvent::Result(result) => {
                    if let Ok(json) = serde_json::to_string_pretty(&result.raw_json) {
                        self.append_log_kind(
                            format!("登录成功，接口返回数据：\n{json}"),
                            LogKind::Success,
                        );
                    } else {
                        self.append_log_kind("登录成功，但结果 JSON 格式化失败。", LogKind::Warn);
                    }
                    self.result = Some(result);
                }
                LoginEvent::Error(message) => {
                    self.status = LoginStatus::Failed;
                    self.status_detail = message.clone();
                    self.append_log_kind(format!("错误：{message}"), LogKind::Error);
                }
                LoginEvent::Finished => {
                    self.task = None;
                }
            }
            ctx.request_repaint();
        }
    }

    fn load_qr_texture(&mut self, ctx: &egui::Context, qr: QrCodeInfo) {
        self.qr_base64_len = qr.base64_png.len();
        match image::load_from_memory(&qr.png_bytes) {
            Ok(image) => {
                let rgba = image.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.qr_texture =
                    Some(ctx.load_texture("qq-login-qrcode", color_image, Default::default()));
                self.qr_loaded_at = Some(Instant::now());
            }
            Err(err) => {
                self.status = LoginStatus::Failed;
                self.status_detail = format!("二维码图片解析失败: {err}");
                self.append_log_kind(&self.status_detail.clone(), LogKind::Error);
            }
        }
    }

    fn append_log_kind(&mut self, line: impl Into<String>, kind: LogKind) {
        let line = line.into();
        for line in line.lines() {
            self.logs.push(LogEntry {
                time: current_time_hms(),
                line: line.to_owned(),
                kind,
            });
        }
        if self.logs.len() > 500 {
            let drain_count = self.logs.len() - 500;
            self.logs.drain(0..drain_count);
        }
    }

    fn logs_text(&self) -> String {
        self.logs
            .iter()
            .map(|entry| format!("[{}] {}", entry.time, entry.line))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn qr_remaining(&self) -> Option<Duration> {
        if matches!(
            self.status,
            LoginStatus::Expired
                | LoginStatus::Rejected
                | LoginStatus::Success
                | LoginStatus::Failed
        ) {
            return Some(Duration::ZERO);
        }

        self.qr_loaded_at
            .map(|loaded_at| QR_TTL.saturating_sub(loaded_at.elapsed()))
    }

    fn append_status_log(&mut self, status: &LoginStatus) {
        let message = match status {
            LoginStatus::Idle => return,
            LoginStatus::RequestingQrCode => "二维码状态：正在获取二维码",
            LoginStatus::WaitingScan => "二维码状态：等待扫码",
            LoginStatus::Authenticating => "二维码状态：已扫码，等待手机确认",
            LoginStatus::Rejected => "二维码状态：本次登录已取消",
            LoginStatus::Expired => "二维码状态：二维码已过期",
            LoginStatus::Authorizing => "登录流程：扫码成功，正在完成道聚城授权",
            LoginStatus::Success => "登录流程：登录成功",
            LoginStatus::Failed => "登录流程：登录失败",
        };
        let kind = match status {
            LoginStatus::Success => LogKind::Success,
            LoginStatus::Rejected | LoginStatus::Expired | LoginStatus::Failed => LogKind::Warn,
            LoginStatus::RequestingQrCode => LogKind::Muted,
            _ => LogKind::Info,
        };
        self.append_log_kind(message, kind);
    }
}

fn current_time_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn install_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.window_margin = Margin::same(0);
    style.visuals.window_fill = tok::BG_SOFT;
    style.visuals.panel_fill = tok::BG_SOFT;

    let widgets = &mut style.visuals.widgets;
    for s in [
        &mut widgets.inactive,
        &mut widgets.hovered,
        &mut widgets.active,
        &mut widgets.open,
        &mut widgets.noninteractive,
    ] {
        s.corner_radius = CornerRadius::same(8);
        s.bg_stroke = Stroke::NONE;
    }
    widgets.inactive.bg_fill = tok::BG_MUTE;
    widgets.hovered.bg_fill = tok::BG_HOVER;
    widgets.active.bg_fill = Color32::from_rgb(208, 208, 208);

    style.visuals.selection.bg_fill = tok::ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0, tok::ACCENT);
    style.visuals.hyperlink_color = tok::ACCENT;

    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(16.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(13.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        FontId::new(12.0, FontFamily::Monospace),
    );

    ctx.set_style(style);
}

fn install_chinese_font(ctx: &egui::Context) {
    const FONT_PATHS: &[&str] = &[
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
    ];

    let Some((path, bytes)) = FONT_PATHS
        .iter()
        .find_map(|path| fs::read(path).ok().map(|bytes| (*path, bytes)))
    else {
        tracing::warn!("no Chinese font found, egui may render CJK text as boxes");
        return;
    };

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("cjk".to_owned(), FontData::from_owned(bytes).into());

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());

    ctx.set_fonts(fonts);
    tracing::info!("installed egui CJK font: {path}");
}

impl eframe::App for DaojuLoginApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.auto_started {
            self.auto_started = true;
            self.start_login();
        }

        self.drain_events(ctx);

        // Drive the countdown digit, the blinking cursor, and the pulse-ring
        // animations off a steady half-second tick.
        ctx.request_repaint_after(Duration::from_millis(500));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::default()
                    .fill(tok::BG_SOFT)
                    .inner_margin(Margin::same(0)),
            )
            .show(ctx, |ui| {
                egui::Frame::default()
                    .fill(tok::BG)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(0))
                    .inner_margin(Margin::same(0))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        render_top_area(ui, self);
                        render_log_panel(ui, self);
                    });
            });
    }
}

impl Drop for DaojuLoginApp {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn render_top_area(ui: &mut egui::Ui, app: &mut DaojuLoginApp) {
    let top_height = 220.0;
    let total_width = ui.available_width();
    let area_top = ui.cursor().top();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;

        ui.allocate_ui_with_layout(
            Vec2::new(200.0, top_height),
            Layout::top_down(Align::Center),
            |ui| render_qr_section(ui, app, top_height),
        );

        let sep_x = ui.cursor().left() + 0.5;
        ui.painter().vline(
            sep_x,
            area_top..=area_top + top_height,
            Stroke::new(1.0, tok::BORDER_SOFT),
        );

        let avail = (total_width - 200.0).max(360.0);
        ui.allocate_ui_with_layout(
            Vec2::new(avail, top_height),
            Layout::top_down(Align::Min),
            |ui| render_operation_section(ui, app, top_height),
        );
    });

    ui.painter().hline(
        ui.min_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, tok::BORDER_SOFT),
    );
}

fn render_qr_section(ui: &mut egui::Ui, app: &DaojuLoginApp, height: f32) {
    egui::Frame::default()
        .fill(tok::BG_SOFT)
        .inner_margin(Margin::symmetric(20, 24))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(160.0, height));
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("扫 码 登 录")
                        .font(FontId::new(11.0, FontFamily::Proportional))
                        .strong()
                        .color(tok::TEXT_FAINT),
                );
                ui.add_space(12.0);

                egui::Frame::default()
                    .fill(tok::BG)
                    .stroke(Stroke::new(1.0, tok::BORDER))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        let inner = Vec2::splat(124.0);
                        if let Some(texture) = &app.qr_texture {
                            let resp = ui.image((texture.id(), inner));

                            if matches!(app.status, LoginStatus::Expired) {
                                paint_expired_overlay(ui, resp.rect);
                            }
                        } else {
                            ui.set_min_size(inner);
                            ui.centered_and_justified(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add(egui::Spinner::new().size(22.0).color(tok::ACCENT));
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("获取中...")
                                            .small()
                                            .color(tok::TEXT_MUTE),
                                    );
                                });
                            });
                        }
                    });

                ui.add_space(12.0);
                match app.qr_remaining() {
                    Some(remaining) if remaining > Duration::ZERO => {
                        let secs = remaining.as_secs();
                        let color = if secs < 30 {
                            tok::WARN
                        } else {
                            tok::TEXT_FAINT
                        };
                        ui.label(
                            RichText::new(format!("{:02}:{:02} 后过期", secs / 60, secs % 60))
                                .font(FontId::new(11.0, FontFamily::Monospace))
                                .color(color),
                        );
                    }
                    Some(_) if matches!(app.status, LoginStatus::Expired) => {
                        ui.label(
                            RichText::new("二维码已失效")
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .strong()
                                .color(tok::DANGER),
                        );
                    }
                    _ => {
                        ui.label(
                            RichText::new("软件打开后自动获取")
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .color(tok::TEXT_FAINT),
                        );
                    }
                }
            });
        });
}

fn paint_expired_overlay(ui: &mut egui::Ui, rect: egui::Rect) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(8),
        Color32::from_rgba_unmultiplied(255, 255, 255, 230),
    );

    let center = rect.center() - Vec2::new(0.0, 8.0);

    // Pulse-ring: a fading expanding circle, looped every 2 seconds.
    let t = (ui.ctx().input(|i| i.time) % 2.0) / 2.0;
    let pulse_r = 22.0 + (t as f32) * 14.0;
    let pulse_a = ((1.0 - t as f32) * 110.0).max(0.0) as u8;
    painter.circle_stroke(
        center,
        pulse_r,
        Stroke::new(
            2.0,
            Color32::from_rgba_unmultiplied(
                tok::ACCENT.r(),
                tok::ACCENT.g(),
                tok::ACCENT.b(),
                pulse_a,
            ),
        ),
    );

    painter.circle_filled(center, 18.0, tok::ACCENT);
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        "↻",
        FontId::new(18.0, FontFamily::Proportional),
        Color32::WHITE,
    );
    painter.text(
        rect.center() + Vec2::new(0.0, 22.0),
        egui::Align2::CENTER_CENTER,
        "已过期，点击刷新",
        FontId::new(11.0, FontFamily::Proportional),
        tok::TEXT_SOFT,
    );
}

fn render_operation_section(ui: &mut egui::Ui, app: &mut DaojuLoginApp, height: f32) {
    egui::Frame::default()
        .fill(tok::BG)
        .inner_margin(Margin::symmetric(24, 20))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(ui.available_width(), height));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("操作")
                            .font(FontId::new(16.0, FontFamily::Proportional))
                            .strong()
                            .color(tok::TEXT),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new("使用 QQ App 扫描左侧二维码")
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .color(tok::TEXT_FAINT),
                        );
                    });
                });
                ui.add_space(14.0);
                render_status_row(ui, app);
                ui.add_space(14.0);
                render_buttons_row(ui, app);
                ui.add_space(12.0);
                ui.label(
                    RichText::new("打开 QQ → 扫一扫 → 扫描二维码 → 确认登录")
                        .font(FontId::new(12.0, FontFamily::Proportional))
                        .color(tok::TEXT_FAINT),
                );
            });
        });
}

fn render_status_row(ui: &mut egui::Ui, app: &DaojuLoginApp) {
    let color = status_color(&app.status);
    let bg = status_background(&app.status);
    let border = status_border(&app.status);

    egui::Frame::default()
        .fill(bg)
        .stroke(Stroke::new(1.0, border))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;

                let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius::same(8),
                    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 28),
                );
                ui.painter().circle_filled(rect.center(), 3.0, color);

                ui.label(
                    RichText::new(app.status.text())
                        .font(FontId::new(13.0, FontFamily::Proportional))
                        .strong()
                        .color(color),
                );

                if matches!(
                    app.status,
                    LoginStatus::RequestingQrCode | LoginStatus::Authorizing
                ) {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(egui::Spinner::new().size(14.0).color(color));
                    });
                }
            });
            if !app.status_detail.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(&app.status_detail)
                        .small()
                        .color(tok::TEXT_MUTE),
                );
            }
        });
}

fn render_buttons_row(ui: &mut egui::Ui, app: &mut DaojuLoginApp) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        let refresh_enabled = !matches!(app.status, LoginStatus::RequestingQrCode);
        let refresh_text = if refresh_enabled {
            "刷新二维码"
        } else {
            "获取中..."
        };
        if ui
            .add_enabled(
                refresh_enabled,
                egui::Button::new(RichText::new(refresh_text).strong().color(Color32::WHITE))
                    .fill(tok::ACCENT)
                    .stroke(Stroke::NONE)
                    .corner_radius(CornerRadius::same(8))
                    .min_size(Vec2::new(108.0, 32.0)),
            )
            .clicked()
        {
            app.start_login();
        }

        if secondary_button(ui, "清空日志").clicked() {
            app.logs.clear();
            app.append_log_kind("日志已清空。", LogKind::Muted);
        }

        let copied = app
            .copied_logs_until
            .is_some_and(|until| Instant::now() < until);
        if secondary_button(ui, if copied { "已复制 ✓" } else { "复制日志" }).clicked() {
            ui.ctx().copy_text(app.logs_text());
            app.copied_logs_until = Some(Instant::now() + Duration::from_millis(1800));
        }

        if let Some(result) = &app.result {
            if secondary_button(ui, "复制登录 JSON").clicked() {
                if let Ok(json) = serde_json::to_string_pretty(&result.raw_json) {
                    ui.ctx().copy_text(json);
                }
            }
        }
    });
}

fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(text).color(tok::TEXT_SOFT))
            .fill(tok::BG_MUTE)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(8))
            .min_size(Vec2::new(86.0, 32.0)),
    )
}

fn render_log_panel(ui: &mut egui::Ui, app: &mut DaojuLoginApp) {
    egui::Frame::default()
        .fill(tok::BG)
        .inner_margin(Margin::symmetric(20, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(">_  日志输出")
                        .font(FontId::new(13.0, FontFamily::Proportional))
                        .strong()
                        .color(tok::TEXT_SOFT),
                );

                ui.add_space(2.0);
                egui::Frame::default()
                    .fill(tok::BG_MUTE)
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(format!("{} 条", app.logs.len()))
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .color(tok::TEXT_MUTE),
                        );
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("清空")
                                    .font(FontId::new(11.0, FontFamily::Proportional))
                                    .color(tok::TEXT_FAINT),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::NONE),
                        )
                        .clicked()
                    {
                        app.logs.clear();
                    }
                });
            });
        });

    ui.painter().hline(
        ui.min_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, tok::BORDER_SOFT),
    );

    egui::Frame::default()
        .fill(tok::CODE_BG)
        .inner_margin(Margin::symmetric(20, 12))
        .show(ui, |ui| {
            ui.set_min_height(ui.available_height().max(220.0));

            let blink = (ui.ctx().input(|i| i.time) % 1.0) < 0.5;

            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if app.logs.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(72.0);
                            ui.label(
                                RichText::new("暂无日志")
                                    .font(FontId::new(12.0, FontFamily::Monospace))
                                    .color(tok::CODE_TIME),
                            );
                        });
                    }

                    let last_index = app.logs.len().saturating_sub(1);
                    for (index, entry) in app.logs.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            ui.label(
                                RichText::new(&entry.time)
                                    .font(FontId::new(11.0, FontFamily::Monospace))
                                    .color(tok::CODE_TIME),
                            );
                            let resp = ui.label(
                                RichText::new(&entry.line)
                                    .font(FontId::new(12.0, FontFamily::Monospace))
                                    .color(log_color(entry.kind)),
                            );

                            if index == last_index && blink {
                                let r = resp.rect;
                                let cursor_x = r.right() + 4.0;
                                let cursor_y = r.center().y;
                                ui.painter().rect_filled(
                                    egui::Rect::from_center_size(
                                        egui::pos2(cursor_x, cursor_y),
                                        Vec2::new(2.0, 12.0),
                                    ),
                                    CornerRadius::same(0),
                                    tok::CODE_SUCCESS,
                                );
                            }
                        });
                    }
                });
        });
}

fn log_color(kind: LogKind) -> Color32 {
    match kind {
        LogKind::Info => tok::CODE_FG,
        LogKind::Success => tok::CODE_SUCCESS,
        LogKind::Warn => tok::WARN,
        LogKind::Error => tok::DANGER,
        LogKind::Muted => tok::CODE_TIME,
    }
}

fn status_color(status: &LoginStatus) -> Color32 {
    match status {
        LoginStatus::Idle | LoginStatus::RequestingQrCode => tok::NEUTRAL,
        LoginStatus::WaitingScan => tok::INFO,
        LoginStatus::Authenticating | LoginStatus::Authorizing => tok::WARN,
        LoginStatus::Success => tok::ACCENT,
        LoginStatus::Failed | LoginStatus::Rejected | LoginStatus::Expired => tok::DANGER,
    }
}

fn status_background(status: &LoginStatus) -> Color32 {
    let c = status_color(status);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 20)
}

fn status_border(status: &LoginStatus) -> Color32 {
    let c = status_color(status);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 48)
}
