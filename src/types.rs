use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct QrCodeInfo {
    pub png_bytes: Vec<u8>,
    pub base64_png: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStatus {
    Idle,
    RequestingQrCode,
    WaitingScan,
    Authenticating,
    Rejected,
    Expired,
    Authorizing,
    Success,
    Failed,
}

impl LoginStatus {
    pub fn text(&self) -> &'static str {
        match self {
            Self::Idle => "准备就绪",
            Self::RequestingQrCode => "正在获取二维码...",
            Self::WaitingScan => "请使用 QQ 扫码登录",
            Self::Authenticating => "已扫码，请在手机上确认",
            Self::Rejected => "登录已取消",
            Self::Expired => "二维码已过期，请重新获取",
            Self::Authorizing => "扫码成功，正在完成道聚城授权...",
            Self::Success => "登录成功",
            Self::Failed => "登录失败",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResult {
    pub account: Value,
    pub raw_json: Value,
}

#[derive(Debug, Clone)]
pub enum LoginEvent {
    Status(LoginStatus),
    QrCode(QrCodeInfo),
    Result(LoginResult),
    Error(String),
    Finished,
}

#[derive(Debug, Clone)]
pub enum PollStatus {
    NotExpired,
    Authenticating,
    Rejected,
    Expired,
    Success { redirect_url: String },
    Error(String),
}
