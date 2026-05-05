use crate::login_service;
use crate::types::{
    AccountSession, LoginEvent, LoginResult, LoginStatus, QrCodeInfo, UserInfo, UserInfoEvent,
};
use crate::user_service;
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

// Design tokens — mirror `colors_and_type.css` from the Claude Design bundle.
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
    pub const ACCENT_HOVER: Color32 = Color32::from_rgb(14, 140, 108);
    pub const INFO: Color32 = Color32::from_rgb(59, 130, 246);
    pub const WARN: Color32 = Color32::from_rgb(217, 119, 6);
    pub const DANGER: Color32 = Color32::from_rgb(239, 68, 68);
    pub const NEUTRAL: Color32 = Color32::from_rgb(142, 142, 160);
    pub const CODE_BG: Color32 = Color32::from_rgb(31, 35, 41);
    pub const CODE_FG: Color32 = Color32::from_rgb(230, 230, 230);
    pub const CODE_TIME: Color32 = Color32::from_rgb(86, 88, 105);
    pub const CODE_SUCCESS: Color32 = Color32::from_rgb(25, 195, 125);
}

// Hand-drawn line icons matching the design's stroke aesthetic. Each function
// fits the icon inside the supplied `Rect` and uses the supplied color.
mod icons {
    use eframe::egui::{
        pos2, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2,
    };
    use std::f32::consts::PI;

    pub fn refresh(p: &Painter, r: Rect, color: Color32) {
        let sw = (r.width() / 12.0).max(1.3);
        let stroke = Stroke::new(sw, color);
        let c = r.center();
        let radius = r.width() * 0.34;

        // ~306° arc; the arrowhead at the end fills the gap.
        let start_a = 0.0_f32;
        let end_a = 1.7 * PI;
        let segs = 28;
        let pts: Vec<Pos2> = (0..=segs)
            .map(|i| {
                let t = i as f32 / segs as f32;
                let a = start_a + (end_a - start_a) * t;
                c + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        p.add(Shape::line(pts, stroke));

        // Arrowhead at the arc's end, pointing along the tangent (the
        // direction the arc is heading at that point).
        let tip = c + Vec2::new(end_a.cos(), end_a.sin()) * radius;
        let tangent = Vec2::new(-end_a.sin(), end_a.cos());
        let radial = Vec2::new(end_a.cos(), end_a.sin());
        let asize = sw * 2.4;

        let apex = tip + tangent * asize;
        let wing_outer = tip + radial * asize * 0.8;
        let wing_inner = tip - radial * asize * 0.8;
        p.add(Shape::convex_polygon(
            vec![apex, wing_outer, wing_inner],
            color,
            Stroke::NONE,
        ));
    }

    pub fn loading(p: &Painter, r: Rect, color: Color32, time: f64) {
        let sw = (r.width() / 11.0).max(1.5);
        let radius = r.width() * 0.36;
        let c = r.center();
        // Faint background ring.
        p.circle_stroke(
            c,
            radius,
            Stroke::new(
                sw,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 80),
            ),
        );
        // Foreground arc rotating once per ~0.7s, like the design's spinner.
        let rot = (time * 9.0) as f32;
        let arc_len = PI * 0.6;
        let segs = 18;
        let pts: Vec<Pos2> = (0..=segs)
            .map(|i| {
                let t = i as f32 / segs as f32;
                let a = rot + arc_len * t;
                c + Vec2::new(a.cos(), a.sin()) * radius
            })
            .collect();
        p.add(Shape::line(pts, Stroke::new(sw, color)));
    }

    pub fn trash(p: &Painter, r: Rect, color: Color32) {
        let sw = (r.width() / 12.0).max(1.4);
        let stroke = Stroke::new(sw, color);
        let c = r.center();
        let w = r.width() * 0.55;
        let h = r.height() * 0.6;
        let top = c.y - h * 0.3;
        let bot = c.y + h * 0.5;
        let left = c.x - w * 0.5;
        let right = c.x + w * 0.5;

        // Lid line.
        p.line_segment(
            [pos2(left - 1.5, top), pos2(right + 1.5, top)],
            stroke,
        );
        // Handle bracket above the lid.
        let hw = w * 0.25;
        p.line_segment(
            [pos2(c.x - hw, top - 2.5), pos2(c.x + hw, top - 2.5)],
            stroke,
        );
        p.line_segment([pos2(c.x - hw, top - 2.5), pos2(c.x - hw, top)], stroke);
        p.line_segment([pos2(c.x + hw, top - 2.5), pos2(c.x + hw, top)], stroke);
        // Body sides + bottom.
        p.line_segment(
            [pos2(left + 1.0, top + 1.5), pos2(left + 1.5, bot)],
            stroke,
        );
        p.line_segment(
            [pos2(right - 1.0, top + 1.5), pos2(right - 1.5, bot)],
            stroke,
        );
        p.line_segment([pos2(left + 1.5, bot), pos2(right - 1.5, bot)], stroke);
    }

    pub fn copy(p: &Painter, r: Rect, color: Color32) {
        let sw = (r.width() / 12.0).max(1.3);
        let stroke = Stroke::new(sw, color);
        let c = r.center();
        let s = r.width() * 0.46;
        let off = s * 0.3;

        // Front (foreground) rounded square, shifted toward lower-right.
        let front = Rect::from_center_size(c + Vec2::splat(off * 0.5), Vec2::splat(s));
        p.rect_stroke(front, 1.5, stroke, StrokeKind::Middle);

        // Back square: only its top + left edges show (the L behind front).
        let back_tl = front.left_top() - Vec2::splat(off);
        let back_tr = pos2(back_tl.x + s, back_tl.y);
        let back_bl = pos2(back_tl.x, back_tl.y + s);
        p.line_segment([back_tl, back_tr], stroke);
        p.line_segment([back_tl, back_bl], stroke);
    }

    pub fn terminal(p: &Painter, r: Rect, color: Color32) {
        let sw = (r.width() / 11.0).max(1.6);
        let stroke = Stroke::new(sw, color);
        let c = r.center();
        let s = r.width() * 0.32;
        // ">" chevron.
        p.line_segment(
            [c + Vec2::new(-s, -s * 0.85), c + Vec2::new(0.0, 0.0)],
            stroke,
        );
        p.line_segment(
            [c + Vec2::new(0.0, 0.0), c + Vec2::new(-s, s * 0.85)],
            stroke,
        );
        // Underscore.
        p.line_segment(
            [
                c + Vec2::new(s * 0.1, s * 0.85),
                c + Vec2::new(s * 0.95, s * 0.85),
            ],
            stroke,
        );
    }

    pub fn scan(p: &Painter, r: Rect, color: Color32) {
        let sw = (r.width() / 12.0).max(1.4);
        let stroke = Stroke::new(sw, color);
        let c = r.center();
        let s = r.width() * 0.4;
        let cn = s * 0.42;
        // Four L-shaped corners.
        for &(sx, sy) in &[(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let corner = c + Vec2::new(sx * s, sy * s);
            p.line_segment([corner, corner + Vec2::new(-sx * cn, 0.0)], stroke);
            p.line_segment([corner, corner + Vec2::new(0.0, -sy * cn)], stroke);
        }
        // Center horizontal scan line.
        p.line_segment([c + Vec2::new(-s, 0.0), c + Vec2::new(s, 0.0)], stroke);
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum BtnStyle {
    Primary,
    Secondary,
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
    session: Option<AccountSession>,
    user_info: Option<UserInfo>,
    user_info_rx: Option<UnboundedReceiver<UserInfoEvent>>,
    user_info_task: Option<JoinHandle<()>>,
    user_info_refreshing: bool,
    avatar_texture: Option<TextureHandle>,
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
            session: None,
            user_info: None,
            user_info_rx: None,
            user_info_task: None,
            user_info_refreshing: false,
            avatar_texture: None,
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
        self.session = None;
        self.user_info = None;
        self.avatar_texture = None;
        self.user_info_refreshing = false;
        if let Some(t) = self.user_info_task.take() {
            t.abort();
        }
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
                    let session = result.session(login_service::APP_ID);
                    self.result = Some(result);
                    if let Some(s) = session {
                        self.append_log_kind(
                            format!(
                                "已保存会话凭证：openid={}, access_token={}…",
                                s.openid,
                                &s.access_token[..8.min(s.access_token.len())]
                            ),
                            LogKind::Info,
                        );
                        self.session = Some(s);
                        self.start_user_info_fetch();
                    } else {
                        self.append_log_kind(
                            "登录返回缺少 access_token / openid，无法保存会话。",
                            LogKind::Warn,
                        );
                    }
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

    fn qr_unusable(&self) -> bool {
        matches!(
            self.status,
            LoginStatus::Expired | LoginStatus::Rejected | LoginStatus::Failed
        )
    }

    fn qr_unusable_caption(&self) -> &'static str {
        match self.status {
            LoginStatus::Expired => "二维码已失效",
            LoginStatus::Rejected => "本次登录已取消",
            LoginStatus::Failed => "登录失败",
            _ => "",
        }
    }

    fn is_logged_in(&self) -> bool {
        self.session.is_some()
    }

    fn start_user_info_fetch(&mut self) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if let Some(task) = self.user_info_task.take() {
            task.abort();
        }
        self.user_info_refreshing = true;
        self.append_log_kind("正在获取用户信息...", LogKind::Info);

        let (tx, rx) = unbounded_channel();
        self.user_info_rx = Some(rx);
        self.user_info_task = Some(self.runtime.spawn(async move {
            user_service::fetch_user_info(session, tx).await;
        }));
    }

    fn drain_user_info_events(&mut self, ctx: &egui::Context) {
        let Some(rx) = &mut self.user_info_rx else {
            return;
        };
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        for event in events {
            match event {
                UserInfoEvent::Loaded(info) => {
                    self.user_info_refreshing = false;
                    self.load_avatar_texture(ctx, &info.avatar_bytes);
                    self.append_log_kind(
                        format!("用户信息已更新：{}", info.nickname),
                        LogKind::Success,
                    );
                    self.user_info = Some(info);
                }
                UserInfoEvent::Error(message) => {
                    self.user_info_refreshing = false;
                    self.append_log_kind(
                        format!("用户信息获取失败：{message}"),
                        LogKind::Error,
                    );
                }
            }
            ctx.request_repaint();
        }
    }

    fn load_avatar_texture(&mut self, ctx: &egui::Context, bytes: &[u8]) {
        match image::load_from_memory(bytes) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let size = [rgba.width() as usize, rgba.height() as usize];
                let pixels = rgba.into_raw();
                let color_image = ColorImage::from_rgba_unmultiplied(size, &pixels);
                self.avatar_texture =
                    Some(ctx.load_texture("avatar", color_image, Default::default()));
            }
            Err(err) => {
                self.append_log_kind(format!("头像加载失败：{err}"), LogKind::Warn);
            }
        }
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
        self.drain_user_info_events(ctx);

        // Drive countdown digits, blinking cursor, pulse-ring, and the
        // in-button spinner off a steady tick.
        ctx.request_repaint_after(Duration::from_millis(50));

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
    let logged_in = app.is_logged_in();

    let mut click_to_refresh = false;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;

        ui.allocate_ui_with_layout(
            Vec2::new(200.0, top_height),
            Layout::top_down(Align::Center),
            |ui| {
                if logged_in {
                    render_account_section(ui, app, top_height);
                } else if render_qr_section(ui, app, top_height) {
                    click_to_refresh = true;
                }
            },
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
            |ui| {
                if logged_in {
                    render_logged_in_section(ui, app, top_height);
                } else {
                    render_operation_section(ui, app, top_height);
                }
            },
        );
    });

    ui.painter().hline(
        ui.min_rect().x_range(),
        ui.cursor().top(),
        Stroke::new(1.0, tok::BORDER_SOFT),
    );

    if click_to_refresh {
        app.start_login();
    }
}

/// Returns `true` if the user clicked the QR card while it was unusable.
fn render_qr_section(ui: &mut egui::Ui, app: &DaojuLoginApp, height: f32) -> bool {
    let unusable = app.qr_unusable();
    let mut clicked_to_refresh = false;

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
                            let sense = if unusable {
                                Sense::click()
                            } else {
                                Sense::hover()
                            };
                            let resp = ui.add(
                                egui::Image::new((texture.id(), inner)).sense(sense),
                            );

                            if unusable {
                                paint_unusable_overlay(ui, resp.rect, app.qr_unusable_caption());
                                if resp.clicked() {
                                    clicked_to_refresh = true;
                                }
                                if resp.hovered() {
                                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                }
                            }
                        } else {
                            ui.allocate_ui_with_layout(
                                inner,
                                Layout::top_down(Align::Center),
                                |ui| {
                                    ui.set_min_size(inner);
                                    let content_h = 22.0 + 8.0 + 14.0;
                                    let pad = ((inner.y - content_h) * 0.5).max(0.0);
                                    ui.add_space(pad);
                                    ui.add(
                                        egui::Spinner::new()
                                            .size(22.0)
                                            .color(tok::ACCENT),
                                    );
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new("获取中...")
                                            .small()
                                            .color(tok::TEXT_MUTE),
                                    );
                                },
                            );
                        }
                    });

                ui.add_space(12.0);
                if unusable && app.qr_texture.is_some() {
                    ui.label(
                        RichText::new(app.qr_unusable_caption())
                            .font(FontId::new(11.0, FontFamily::Proportional))
                            .strong()
                            .color(tok::DANGER),
                    );
                } else if let Some(remaining) = app.qr_remaining() {
                    if remaining > Duration::ZERO {
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
                    } else {
                        ui.label(
                            RichText::new("软件打开后自动获取")
                                .font(FontId::new(11.0, FontFamily::Proportional))
                                .color(tok::TEXT_FAINT),
                        );
                    }
                } else {
                    ui.label(
                        RichText::new("软件打开后自动获取")
                            .font(FontId::new(11.0, FontFamily::Proportional))
                            .color(tok::TEXT_FAINT),
                    );
                }
            });
        });

    clicked_to_refresh
}

fn render_account_section(ui: &mut egui::Ui, app: &DaojuLoginApp, height: f32) {
    egui::Frame::default()
        .fill(tok::BG_SOFT)
        .inner_margin(Margin::symmetric(20, 24))
        .show(ui, |ui| {
            ui.set_min_size(Vec2::new(160.0, height));
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("已 登 录")
                        .font(FontId::new(11.0, FontFamily::Proportional))
                        .strong()
                        .color(tok::ACCENT),
                );
                ui.add_space(12.0);

                let avatar_size = Vec2::splat(96.0);
                if let Some(tex) = &app.avatar_texture {
                    ui.add(
                        egui::Image::new((tex.id(), avatar_size))
                            .corner_radius(48),
                    );
                } else {
                    ui.allocate_ui_with_layout(
                        avatar_size,
                        Layout::top_down(Align::Center),
                        |ui| {
                            ui.set_min_size(avatar_size);
                            ui.add_space((avatar_size.y - 22.0) * 0.5);
                            ui.add(
                                egui::Spinner::new()
                                    .size(22.0)
                                    .color(tok::ACCENT),
                            );
                        },
                    );
                }

                ui.add_space(12.0);
                if let Some(info) = &app.user_info {
                    ui.label(
                        RichText::new(&info.nickname)
                            .font(FontId::new(14.0, FontFamily::Proportional))
                            .strong()
                            .color(tok::TEXT),
                    );
                } else if app.user_info_refreshing {
                    ui.label(
                        RichText::new("加载用户信息...")
                            .font(FontId::new(11.0, FontFamily::Proportional))
                            .color(tok::TEXT_MUTE),
                    );
                } else {
                    ui.label(
                        RichText::new("点击「刷新用户信息」")
                            .font(FontId::new(11.0, FontFamily::Proportional))
                            .color(tok::TEXT_MUTE),
                    );
                }
            });
        });
}

fn render_logged_in_section(ui: &mut egui::Ui, app: &mut DaojuLoginApp, height: f32) {
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
                        if let Some(session) = &app.session {
                            let openid = &session.openid;
                            let short = if openid.len() > 12 {
                                format!("{}…{}", &openid[..6], &openid[openid.len() - 4..])
                            } else {
                                openid.clone()
                            };
                            ui.label(
                                RichText::new(format!("openid {short}"))
                                    .font(FontId::new(11.0, FontFamily::Monospace))
                                    .color(tok::TEXT_FAINT),
                            );
                        }
                    });
                });
                ui.add_space(14.0);
                render_status_row(ui, app);
                ui.add_space(14.0);

                let refreshing = app.user_info_refreshing;
                let refresh_text = if refreshing {
                    "正在刷新..."
                } else {
                    "刷新用户信息"
                };
                let time = ui.ctx().input(|i| i.time);
                let resp = icon_button(
                    ui,
                    refresh_text,
                    BtnStyle::Primary,
                    !refreshing,
                    |p, r, c| {
                        if refreshing {
                            icons::loading(p, r, c, time);
                        } else {
                            icons::refresh(p, r, c);
                        }
                    },
                );
                if resp.clicked() {
                    app.start_user_info_fetch();
                }

                ui.add_space(12.0);
                if let Some(info) = &app.user_info {
                    let gender = info
                        .raw_json
                        .get("gender")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let province = info
                        .raw_json
                        .get("province")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    let city = info
                        .raw_json
                        .get("city")
                        .and_then(|v| v.as_str())
                        .unwrap_or("-");
                    ui.label(
                        RichText::new(format!("{gender} · {province} · {city}"))
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .color(tok::TEXT_FAINT),
                    );
                } else {
                    ui.label(
                        RichText::new("登录凭证已保存，可用于后续接口调用")
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .color(tok::TEXT_FAINT),
                    );
                }
            });
        });
}

fn paint_unusable_overlay(ui: &mut egui::Ui, rect: egui::Rect, caption: &str) {
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        CornerRadius::same(8),
        Color32::from_rgba_unmultiplied(255, 255, 255, 230),
    );

    let center = rect.center() - Vec2::new(0.0, 8.0);

    // Pulse-ring.
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
    let icon_rect = egui::Rect::from_center_size(center, Vec2::splat(20.0));
    icons::refresh(painter, icon_rect, Color32::WHITE);

    let label = format!("{caption}，点击刷新");
    painter.text(
        rect.center() + Vec2::new(0.0, 24.0),
        egui::Align2::CENTER_CENTER,
        &label,
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
                        ui.add_space(4.0);
                        let (icon_rect, _) =
                            ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                        icons::scan(ui.painter(), icon_rect, tok::TEXT_FAINT);
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

        let refreshing = matches!(app.status, LoginStatus::RequestingQrCode);
        let refresh_text = if refreshing { "获取中..." } else { "刷新二维码" };
        let time = ui.ctx().input(|i| i.time);
        let refresh_resp = icon_button(
            ui,
            refresh_text,
            BtnStyle::Primary,
            !refreshing,
            |p, r, c| {
                if refreshing {
                    icons::loading(p, r, c, time);
                } else {
                    icons::refresh(p, r, c);
                }
            },
        );
        if refresh_resp.clicked() {
            app.start_login();
        }

        if icon_button(ui, "清空日志", BtnStyle::Secondary, true, icons::trash).clicked() {
            app.logs.clear();
            app.append_log_kind("日志已清空。", LogKind::Muted);
        }

        let copied = app
            .copied_logs_until
            .is_some_and(|until| Instant::now() < until);
        let copy_text = if copied { "已复制 ✓" } else { "复制日志" };
        if icon_button(ui, copy_text, BtnStyle::Secondary, true, icons::copy).clicked() {
            ui.ctx().copy_text(app.logs_text());
            app.copied_logs_until = Some(Instant::now() + Duration::from_millis(1800));
        }

        if let Some(result) = &app.result {
            let json = serde_json::to_string_pretty(&result.raw_json).ok();
            if icon_button(ui, "复制 JSON", BtnStyle::Secondary, true, icons::copy).clicked() {
                if let Some(json) = json {
                    ui.ctx().copy_text(json);
                }
            }
        }
    });
}

fn icon_button(
    ui: &mut egui::Ui,
    text: &str,
    style: BtnStyle,
    enabled: bool,
    icon: impl FnOnce(&egui::Painter, egui::Rect, Color32),
) -> egui::Response {
    let (text_color, bg_normal, bg_hover) = match style {
        BtnStyle::Primary => (Color32::WHITE, tok::ACCENT, tok::ACCENT_HOVER),
        BtnStyle::Secondary => (tok::TEXT_SOFT, tok::BG_MUTE, tok::BG_HOVER),
    };
    let icon_size = 13.0;
    let pad_x = 14.0;
    let pad_y = 7.0;
    let gap = 6.0;
    let font = FontId::new(13.0, FontFamily::Proportional);

    let galley = ui.fonts(|f| f.layout_no_wrap(text.to_owned(), font, text_color));
    let content_w = icon_size + gap + galley.size().x;
    let total_w = content_w + pad_x * 2.0;
    let total_h = (galley.size().y.max(icon_size) + pad_y * 2.0).max(32.0);
    let min_w = match style {
        BtnStyle::Primary => 110.0,
        BtnStyle::Secondary => 90.0,
    };
    let final_size = Vec2::new(total_w.max(min_w), total_h);

    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(final_size, sense);

    let (bg, fg) = if !enabled {
        (
            Color32::from_rgba_unmultiplied(
                bg_normal.r(),
                bg_normal.g(),
                bg_normal.b(),
                165,
            ),
            Color32::from_rgba_unmultiplied(
                text_color.r(),
                text_color.g(),
                text_color.b(),
                210,
            ),
        )
    } else if resp.hovered() {
        (bg_hover, text_color)
    } else {
        (bg_normal, text_color)
    };

    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), bg);

    let content_left = rect.center().x - content_w * 0.5;
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(content_left + icon_size * 0.5, rect.center().y),
        Vec2::splat(icon_size),
    );
    icon(ui.painter(), icon_rect, fg);

    let text_pos = egui::pos2(
        content_left + icon_size + gap,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(text_pos, galley, fg);

    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp
}

fn render_log_panel(ui: &mut egui::Ui, app: &mut DaojuLoginApp) {
    egui::Frame::default()
        .fill(tok::BG)
        .inner_margin(Margin::symmetric(20, 10))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) =
                    ui.allocate_exact_size(Vec2::splat(15.0), Sense::hover());
                icons::terminal(ui.painter(), icon_rect, tok::TEXT_SOFT);
                ui.add_space(6.0);
                ui.label(
                    RichText::new("日志输出")
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
