use crate::types::{AccountSession, UserInfo, UserInfoEvent};
use anyhow::{anyhow, Context};
use reqwest::Client;
use serde_json::Value;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{info, warn};

const USER_AGENT_VALUE: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome Safari";

pub async fn fetch_user_info(session: AccountSession, tx: UnboundedSender<UserInfoEvent>) {
    if let Err(err) = fetch_user_info_inner(session, tx.clone()).await {
        warn!("fetch user info failed: {err:#}");
        let _ = tx.send(UserInfoEvent::Error(err.to_string()));
    }
}

async fn fetch_user_info_inner(
    session: AccountSession,
    tx: UnboundedSender<UserInfoEvent>,
) -> anyhow::Result<()> {
    let client = Client::builder()
        .gzip(true)
        .user_agent(USER_AGENT_VALUE)
        .build()
        .context("failed to build user_info http client")?;

    let url = format!(
        "https://graph.qq.com/user/get_user_info?oauth_consumer_key={}&access_token={}&openid={}&format=json",
        session.appid, session.access_token, session.openid,
    );

    let text = client
        .get(&url)
        .send()
        .await
        .context("failed to call get_user_info")?
        .text()
        .await
        .context("failed to read get_user_info response")?;

    let json: Value = serde_json::from_str(&text)
        .with_context(|| format!("failed to parse get_user_info response: {}", compact(&text)))?;

    let ret = json
        .get("ret")
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(-1);
    if ret != 0 {
        let msg = json
            .get("msg")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        anyhow::bail!("get_user_info ret={ret}, msg={msg}");
    }

    let nickname = json
        .get("nickname")
        .and_then(|v| v.as_str())
        .unwrap_or("(无昵称)")
        .to_owned();

    let avatar_url = json
        .get("figureurl_qq_2")
        .or_else(|| json.get("figureurl_qq_1"))
        .or_else(|| json.get("figureurl_2"))
        .or_else(|| json.get("figureurl_qq"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("user_info has no avatar URL"))?
        .to_owned();

    info!("fetched user info, nickname={nickname}");

    let avatar_bytes = client
        .get(&avatar_url)
        .send()
        .await
        .context("failed to download avatar")?
        .bytes()
        .await
        .context("failed to read avatar bytes")?
        .to_vec();

    info!("downloaded avatar, {} bytes", avatar_bytes.len());

    let _ = tx.send(UserInfoEvent::Loaded(UserInfo {
        nickname,
        avatar_bytes,
        raw_json: json,
    }));

    Ok(())
}

fn compact(s: &str) -> String {
    let s: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= 200 {
        s
    } else {
        let prefix: String = s.chars().take(200).collect();
        format!("{prefix}...")
    }
}
