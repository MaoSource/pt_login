use crate::jsonp::{parse_jsonp, parse_ptui_cb_args};
use crate::qq_util::{get_gtk, hash33, uin_to_qq};
use crate::types::{LoginEvent, LoginResult, LoginStatus, PollStatus, QrCodeInfo};
use anyhow::{anyhow, bail, Context};
use base64::Engine;
use reqwest::cookie::Jar;
use reqwest::header::{
    HeaderMap, HeaderValue, ACCEPT_LANGUAGE, CONNECTION, COOKIE, HOST, LOCATION, REFERER,
    USER_AGENT,
};
use reqwest::{Client, StatusCode};
use serde_json::{Map, Value};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::mpsc::UnboundedSender;
use tokio::time::sleep;
use tracing::{info, warn};
use url::Url;

const APP_ID: &str = "1101958653";
const CLIENT_ID: &str = "1101958653";
const REDIRECT_URI: &str = "https://daoju.qq.com/connect/qc_redirect.html";
const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome Safari";

pub async fn run_login_flow(tx: UnboundedSender<LoginEvent>) {
    if let Err(err) = run_login_flow_inner(tx.clone()).await {
        warn!("login flow failed: {err:#}");
        let _ = tx.send(LoginEvent::Status(LoginStatus::Failed));
        let _ = tx.send(LoginEvent::Error(err.to_string()));
    }
    let _ = tx.send(LoginEvent::Finished);
}

async fn run_login_flow_inner(tx: UnboundedSender<LoginEvent>) -> anyhow::Result<()> {
    let clients = HttpClients::new()?;

    let _ = tx.send(LoginEvent::Status(LoginStatus::RequestingQrCode));
    let (qr, qrsig) = fetch_qr_code(&clients.follow_redirects).await?;
    let token = hash33(&qrsig);
    let _ = tx.send(LoginEvent::QrCode(qr));
    let _ = tx.send(LoginEvent::Status(LoginStatus::WaitingScan));

    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(10 * 60) {
        sleep(Duration::from_secs(3)).await;
        match poll_login_status(&clients.follow_redirects, &qrsig, &token).await? {
            PollStatus::NotExpired => {
                let _ = tx.send(LoginEvent::Status(LoginStatus::WaitingScan));
            }
            PollStatus::Authenticating => {
                let _ = tx.send(LoginEvent::Status(LoginStatus::Authenticating));
            }
            PollStatus::Rejected => {
                let _ = tx.send(LoginEvent::Status(LoginStatus::Rejected));
                return Ok(());
            }
            PollStatus::Expired => {
                let _ = tx.send(LoginEvent::Status(LoginStatus::Expired));
                return Ok(());
            }
            PollStatus::Success { redirect_url } => {
                info!("QR login confirmed, starting OAuth authorization");
                let _ = tx.send(LoginEvent::Status(LoginStatus::Authorizing));
                let result = finish_daoju_authorization(&clients, &redirect_url).await?;
                let _ = tx.send(LoginEvent::Result(result));
                let _ = tx.send(LoginEvent::Status(LoginStatus::Success));
                return Ok(());
            }
            PollStatus::Error(message) => bail!(message),
        }
    }

    let _ = tx.send(LoginEvent::Status(LoginStatus::Expired));
    bail!("登录轮询超时，请重新扫码")
}

struct HttpClients {
    follow_redirects: Client,
    no_redirects: Client,
}

impl HttpClients {
    fn new() -> anyhow::Result<Self> {
        let jar = Arc::new(Jar::default());
        let follow_redirects = Client::builder()
            .cookie_provider(jar.clone())
            .gzip(true)
            .user_agent(USER_AGENT_VALUE)
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .context("failed to build HTTP client")?;
        let no_redirects = Client::builder()
            .cookie_provider(jar)
            .gzip(true)
            .user_agent(USER_AGENT_VALUE)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .context("failed to build no-redirect HTTP client")?;

        Ok(Self {
            follow_redirects,
            no_redirects,
        })
    }
}

async fn fetch_qr_code(client: &Client) -> anyhow::Result<(QrCodeInfo, String)> {
    let random = rand::random::<f64>();
    let url = format!(
        "https://ssl.ptlogin2.qq.com/ptqrshow?appid=716027609&e=2&l=M&s=3&d=72&v=4&t={random}&daid=383&pt_3rd_aid={APP_ID}&u1=https%3A%2F%2Fgraph.qq.com%2Foauth2.0%2Flogin_jump"
    );

    let response = client
        .get(url)
        .headers(common_ptlogin_headers()?)
        .send()
        .await
        .context("failed to request QQ QR code")?;

    let qrsig = extract_cookie(response.headers(), "qrsig").context("qrsig cookie not found")?;
    let bytes = response
        .bytes()
        .await
        .context("failed to read QR code bytes")?
        .to_vec();

    if bytes.is_empty() {
        bail!("QQ QR code response is empty");
    }

    let base64_png = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok((
        QrCodeInfo {
            png_bytes: bytes,
            base64_png,
        },
        qrsig,
    ))
}

async fn poll_login_status(
    client: &Client,
    qrsig: &str,
    ptqrtoken: &str,
) -> anyhow::Result<PollStatus> {
    let timestamp = timestamp_millis();
    let url = format!(
        "https://ssl.ptlogin2.qq.com/ptqrlogin?u1=https%3A%2F%2Fgraph.qq.com%2Foauth2.0%2Flogin_jump&ptqrtoken={ptqrtoken}&ptredirect=0&h=1&t=1&g=1&from_ui=1&ptlang=2052&action=0-0-{timestamp}&js_ver=23042119&js_type=1&login_sig=&pt_uistyle=40&aid=716027609&daid=383&pt_3rd_aid={APP_ID}"
    );

    let text = client
        .get(url)
        .headers(common_ptlogin_headers()?)
        .header(COOKIE, format!("qrsig={qrsig}"))
        .send()
        .await
        .context("failed to poll QQ QR login status")?
        .text()
        .await
        .context("failed to read login status response")?;

    info!(
        "poll login status: {}",
        sanitize_poll_response_for_log(&text)
    );
    parse_poll_status(&text)
}

fn parse_poll_status(text: &str) -> anyhow::Result<PollStatus> {
    if text.contains("二维码未失效") {
        return Ok(PollStatus::NotExpired);
    }
    if text.contains("二维码认证中") {
        return Ok(PollStatus::Authenticating);
    }
    if text.contains("本次登录已被拒绝") {
        return Ok(PollStatus::Rejected);
    }
    if text.contains("二维码已失效") {
        return Ok(PollStatus::Expired);
    }
    if text.contains("登录成功") {
        let args = parse_ptui_cb_args(text)?;
        let redirect_url = args
            .get(2)
            .filter(|value| !value.is_empty())
            .cloned()
            .ok_or_else(|| anyhow!("login success response did not include redirect URL"))?;
        return Ok(PollStatus::Success { redirect_url });
    }

    Ok(PollStatus::Error("未知登录状态".to_owned()))
}

async fn finish_daoju_authorization(
    clients: &HttpClients,
    redirect_url: &str,
) -> anyhow::Result<LoginResult> {
    let check_sig_response = clients
        .no_redirects
        .get(redirect_url)
        .headers(graph_headers()?)
        .send()
        .await
        .context("failed to request QQ login redirect URL")?
        .error_for_status()
        .context("QQ login redirect URL returned an error status")?;
    let p_skey = extract_cookie(check_sig_response.headers(), "p_skey")
        .or_else(|| extract_cookie(check_sig_response.headers(), "skey"))
        .ok_or_else(|| anyhow!("check_sig response did not include p_skey cookie"))?;
    info!("check_sig completed, p_skey cookie present");

    let authorize_referer = build_authorize_referer()?;
    let authorize_form = build_authorize_form(&p_skey);
    let response = clients
        .no_redirects
        .post("https://graph.qq.com/oauth2.0/authorize")
        .headers(authorize_headers(&authorize_referer)?)
        .form(&authorize_form)
        .send()
        .await
        .context("failed to request QQ OAuth authorize URL")?;

    if response.status() != StatusCode::FOUND && response.status() != StatusCode::MOVED_PERMANENTLY
    {
        bail!(
            "QQ OAuth authorize did not return redirect Location, status {}",
            response.status()
        );
    }

    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| anyhow!("QQ OAuth authorize response has no Location header"))?;
    info!(
        "QQ OAuth authorize Location: {}",
        sanitize_url_for_log(location)
    );
    let code = extract_query_param(location, "code").context("OAuth Location has no code")?;

    request_daoju_login(&clients.follow_redirects, &code).await
}

fn build_authorize_referer() -> anyhow::Result<Url> {
    let mut url = Url::parse("https://graph.qq.com/oauth2.0/show")?;
    url.query_pairs_mut()
        .append_pair("which", "Login")
        .append_pair("display", "pc")
        .append_pair("response_type", "code")
        .append_pair("state", "STATE")
        .append_pair("client_id", CLIENT_ID)
        .append_pair(
            "redirect_uri",
            "https://daoju.qq.com/connect/qc_redirect.html?parent_domain=https%3A%2F%2Fdaoju.qq.com",
        );
    Ok(url)
}

fn build_authorize_form(p_skey: &str) -> Vec<(&'static str, String)> {
    vec![
        ("response_type", "code".to_owned()),
        ("client_id", CLIENT_ID.to_owned()),
        (
            "redirect_uri",
            "https://daoju.qq.com/connect/qc_redirect.html?parent_domain=https%3A%2F%2Fdaoju.qq.com"
                .to_owned(),
        ),
        ("scope", String::new()),
        ("state", "STATE".to_owned()),
        ("switch", String::new()),
        ("from_ptlogin", "1".to_owned()),
        ("src", "1".to_owned()),
        ("update_auth", "1".to_owned()),
        ("openapi", "1010".to_owned()),
        ("g_tk", get_gtk(p_skey)),
        ("auth_time", timestamp_millis().to_string()),
    ]
}

async fn request_daoju_login(client: &Client, code: &str) -> anyhow::Result<LoginResult> {
    let timestamp = timestamp_millis();
    let callback = format!("jQuery{}_{}", timestamp % 1_000_000, timestamp);
    let mut url = Url::parse("https://ams.game.qq.com/ams/userLoginSvr")?;
    url.query_pairs_mut()
        .append_pair("a", "qcCodeToOpenId")
        .append_pair("qc_code", code)
        .append_pair("appid", APP_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("callback", &callback)
        .append_pair("_", &timestamp.to_string());

    let text = client
        .get(url)
        .header(USER_AGENT, USER_AGENT_VALUE)
        .header(REFERER, "https://daoju.qq.com/")
        .send()
        .await
        .context("failed to request Daoju login API")?
        .text()
        .await
        .context("failed to read Daoju login response")?;

    let raw_json = parse_jsonp(&text).with_context(|| {
        format!(
            "failed to parse Daoju JSONP response, body={}",
            compact_for_log(&text)
        )
    })?;
    let iret_ok = match raw_json.get("iRet") {
        Some(Value::String(value)) => value == "0",
        Some(Value::Number(value)) => value.as_i64() == Some(0),
        _ => false,
    };

    if !iret_ok {
        bail!("道聚城登录失败: {}", raw_json);
    }

    Ok(LoginResult {
        account: normalize_account_json(&raw_json),
        raw_json,
    })
}

fn normalize_account_json(raw: &Value) -> Value {
    let mut account = Map::new();
    for key in [
        "openid",
        "openId",
        "access_token",
        "accessToken",
        "nickName",
        "nickname",
        "uin",
        "p_uin",
        "qq",
    ] {
        if let Some(value) = raw.get(key) {
            account.insert(key.to_owned(), value.clone());
        }
    }

    if let Some(Value::String(p_uin)) = raw.get("p_uin") {
        account.insert("qq".to_owned(), Value::String(uin_to_qq(p_uin)));
    }
    if let Some(Value::String(skey)) = raw.get("skey") {
        account.insert("gtk".to_owned(), Value::String(get_gtk(skey)));
    }

    if account.is_empty() {
        raw.clone()
    } else {
        Value::Object(account)
    }
}

fn common_ptlogin_headers() -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(HOST, HeaderValue::from_static("ssl.ptlogin2.qq.com"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://xui.ptlogin2.qq.com/"),
    );
    headers.insert(CONNECTION, HeaderValue::from_static("keep-alive"));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    Ok(headers)
}

fn graph_headers() -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert(
        REFERER,
        HeaderValue::from_static("https://xui.ptlogin2.qq.com/"),
    );
    Ok(headers)
}

fn authorize_headers(referer: &Url) -> anyhow::Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(USER_AGENT_VALUE));
    headers.insert(ACCEPT_LANGUAGE, HeaderValue::from_static("zh-CN,zh;q=0.9"));
    headers.insert(
        REFERER,
        HeaderValue::from_str(referer.as_str()).context("invalid authorize referer header")?,
    );
    Ok(headers)
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    for value in headers.get_all(reqwest::header::SET_COOKIE) {
        let cookie = value.to_str().ok()?;
        for part in cookie.split(';').take(1) {
            let (cookie_name, cookie_value) = part.split_once('=')?;
            if cookie_name.trim() == name {
                return Some(cookie_value.trim().to_owned());
            }
        }
    }
    None
}

fn extract_query_param(input: &str, key: &str) -> anyhow::Result<String> {
    let url =
        Url::parse(input).or_else(|_| Url::parse(&format!("https://daoju.qq.com/{input}")))?;
    url.query_pairs()
        .find_map(|(name, value)| (name == key).then(|| value.into_owned()))
        .ok_or_else(|| anyhow!("query parameter {key} not found"))
}

fn timestamp_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn classify_response_for_log(text: &str) -> &'static str {
    if text.contains("二维码未失效") {
        "not_expired"
    } else if text.contains("二维码认证中") {
        "authenticating"
    } else if text.contains("本次登录已被拒绝") {
        "rejected"
    } else if text.contains("二维码已失效") {
        "expired"
    } else if text.contains("登录成功") {
        "success"
    } else {
        "unknown"
    }
}

fn sanitize_poll_response_for_log(text: &str) -> String {
    let kind = classify_response_for_log(text);

    if text.contains("登录成功") {
        return match parse_ptui_cb_args(text) {
            Ok(args) => {
                let redirect = args
                    .get(2)
                    .map(|value| sanitize_url_for_log(value))
                    .unwrap_or_else(|| "<missing redirect>".to_owned());
                let message = args.get(4).cloned().unwrap_or_default();
                let nick = args
                    .get(5)
                    .map(|value| mask_for_log(value))
                    .unwrap_or_default();
                format!("kind={kind}, message={message}, nick={nick}, redirect={redirect}")
            }
            Err(_) => format!("kind={kind}, body={}", compact_for_log(text)),
        };
    }

    format!("kind={kind}, body={}", compact_for_log(text))
}

fn sanitize_url_for_log(input: &str) -> String {
    if raw_sensitive_logs_enabled() {
        return input.to_owned();
    }

    if input.is_empty() {
        return "<empty>".to_owned();
    }

    let parsed =
        Url::parse(input).or_else(|_| Url::parse(&format!("https://relative.invalid{input}")));
    let Ok(url) = parsed else {
        return mask_for_log(input);
    };

    let mut output = String::new();
    if url.domain() == Some("relative.invalid") {
        output.push_str(url.path());
    } else {
        output.push_str(url.scheme());
        output.push_str("://");
        output.push_str(url.host_str().unwrap_or("<no-host>"));
        output.push_str(url.path());
    }

    let query_keys: Vec<String> = url.query_pairs().map(|(key, _)| key.into_owned()).collect();
    if !query_keys.is_empty() {
        output.push_str("?<");
        output.push_str(&query_keys.join(","));
        output.push('>');
    }

    output
}

fn compact_for_log(input: &str) -> String {
    if raw_sensitive_logs_enabled() {
        return input.to_owned();
    }

    let compact = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= 500 {
        compact
    } else {
        let prefix: String = compact.chars().take(500).collect();
        format!("{prefix}...")
    }
}

fn mask_for_log(input: &str) -> String {
    if raw_sensitive_logs_enabled() {
        return input.to_owned();
    }

    if input.chars().count() <= 12 {
        return input.to_owned();
    }

    let prefix: String = input.chars().take(4).collect();
    let suffix: String = input
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{prefix}...{suffix}")
}

fn raw_sensitive_logs_enabled() -> bool {
    std::env::var("PT_LOGIN_RAW_LOGS")
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}
