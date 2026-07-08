//! Google Calendar API v3 の薄いラッパー。必要なフィールドだけ受け取る。
//! ドキュメント: https://developers.google.com/calendar/api/v3/reference

use serde::Deserialize;
use serde_json::Value;

/// API 呼び出しのエラー。増分同期トークンの失効（410）は上位でフル同期に切り替える。
#[derive(Debug)]
pub enum ApiError {
    /// syncToken が失効した（410 Gone）。カレンダーをフル同期し直す必要がある。
    SyncTokenExpired,
    /// その他のエラー（メッセージ）。
    Message(String),
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::SyncTokenExpired => write!(f, "同期トークンが失効しました"),
            ApiError::Message(m) => write!(f, "{m}"),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Message(e.to_string())
    }
}

// ── レスポンス型（読み取り用） ────────────────────────────────────────

#[derive(Debug, Deserialize, Default)]
pub struct GCalendarListEntry {
    pub id: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(rename = "backgroundColor", default)]
    pub background_color: Option<String>,
    #[serde(rename = "accessRole", default)]
    pub access_role: Option<String>,
    #[serde(default)]
    pub primary: Option<bool>,
    #[serde(default)]
    pub deleted: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct CalendarListResponse {
    #[serde(default)]
    items: Vec<GCalendarListEntry>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GTime {
    /// 終日: 'YYYY-MM-DD'。
    #[serde(default)]
    pub date: Option<String>,
    /// 時間指定: RFC3339（オフセット付き）。
    #[serde(rename = "dateTime", default)]
    pub date_time: Option<String>,
    #[serde(rename = "timeZone", default)]
    pub time_zone: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GReminderOverride {
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub minutes: Option<i64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GReminders {
    #[serde(rename = "useDefault", default)]
    pub use_default: bool,
    #[serde(default)]
    pub overrides: Option<Vec<GReminderOverride>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GEvent {
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub location: Option<String>,
    #[serde(default)]
    pub start: Option<GTime>,
    #[serde(default)]
    pub end: Option<GTime>,
    #[serde(default)]
    pub recurrence: Option<Vec<String>>,
    #[serde(default)]
    pub transparency: Option<String>,
    #[serde(default)]
    pub visibility: Option<String>,
    #[serde(rename = "recurringEventId", default)]
    pub recurring_event_id: Option<String>,
    #[serde(default)]
    pub reminders: Option<GReminders>,
}

#[derive(Debug, Deserialize, Default)]
pub struct EventsPage {
    #[serde(default)]
    pub items: Vec<GEvent>,
    #[serde(rename = "nextPageToken", default)]
    pub next_page_token: Option<String>,
    #[serde(rename = "nextSyncToken", default)]
    pub next_sync_token: Option<String>,
}

/// パスセグメント用の最小パーセントエンコード（カレンダー ID・予定 ID に @ や : が入る）。
fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'@' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// レスポンスが失敗ステータスなら本文を含むエラーへ変換。410 は SyncTokenExpired。
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    let status = resp.status();
    if status.is_success() {
        return Ok(resp);
    }
    if status.as_u16() == 410 {
        return Err(ApiError::SyncTokenExpired);
    }
    let body = resp.text().await.unwrap_or_default();
    Err(ApiError::Message(format!(
        "Google API エラー（HTTP {}）: {}",
        status.as_u16(),
        body.chars().take(300).collect::<String>()
    )))
}

/// 連携アカウントのカレンダー一覧（ページングを畳んで全件返す）。
pub async fn list_calendars(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<GCalendarListEntry>, ApiError> {
    let mut out = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut query: Vec<(&str, String)> = vec![("maxResults", "250".into())];
        if let Some(pt) = &page_token {
            query.push(("pageToken", pt.clone()));
        }
        let resp = client
            .get(format!("{}/users/me/calendarList", super::CAL_API_BASE))
            .bearer_auth(token)
            .query(&query)
            .send()
            .await?;
        let page: CalendarListResponse = check(resp).await?.json().await?;
        out.extend(page.items);
        match page.next_page_token {
            Some(next) => page_token = Some(next),
            None => break,
        }
    }
    Ok(out)
}

/// 予定一覧を 1 ページ取得する。sync_token があれば増分、なければ time_min からのフル。
/// singleEvents=false / showDeleted=true は増分・フルで一貫させる（syncToken の前提）。
pub async fn list_events(
    client: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    sync_token: Option<&str>,
    time_min: Option<&str>,
    page_token: Option<&str>,
) -> Result<EventsPage, ApiError> {
    let mut query: Vec<(&str, String)> = vec![
        ("singleEvents", "false".into()),
        ("showDeleted", "true".into()),
        ("maxResults", "250".into()),
    ];
    if let Some(st) = sync_token {
        query.push(("syncToken", st.into()));
    } else if let Some(tm) = time_min {
        query.push(("timeMin", tm.into()));
    }
    if let Some(pt) = page_token {
        query.push(("pageToken", pt.into()));
    }
    let resp = client
        .get(format!(
            "{}/calendars/{}/events",
            super::CAL_API_BASE,
            enc(calendar_id)
        ))
        .bearer_auth(token)
        .query(&query)
        .send()
        .await?;
    let page: EventsPage = check(resp).await?.json().await?;
    Ok(page)
}

/// 予定を作成する。
pub async fn insert_event(
    client: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    body: &Value,
) -> Result<GEvent, ApiError> {
    let resp = client
        .post(format!(
            "{}/calendars/{}/events",
            super::CAL_API_BASE,
            enc(calendar_id)
        ))
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let ev: GEvent = check(resp).await?.json().await?;
    Ok(ev)
}

/// 予定を更新する（部分更新 PATCH）。
pub async fn patch_event(
    client: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    event_id: &str,
    body: &Value,
) -> Result<GEvent, ApiError> {
    let resp = client
        .patch(format!(
            "{}/calendars/{}/events/{}",
            super::CAL_API_BASE,
            enc(calendar_id),
            enc(event_id)
        ))
        .bearer_auth(token)
        .json(body)
        .send()
        .await?;
    let ev: GEvent = check(resp).await?.json().await?;
    Ok(ev)
}

/// 予定を削除する。既に無い（404/410）場合は成功扱い。
pub async fn delete_event(
    client: &reqwest::Client,
    token: &str,
    calendar_id: &str,
    event_id: &str,
) -> Result<(), ApiError> {
    let resp = client
        .delete(format!(
            "{}/calendars/{}/events/{}",
            super::CAL_API_BASE,
            enc(calendar_id),
            enc(event_id)
        ))
        .bearer_auth(token)
        .send()
        .await?;
    let status = resp.status();
    if status.is_success() || status.as_u16() == 404 || status.as_u16() == 410 {
        return Ok(());
    }
    let body = resp.text().await.unwrap_or_default();
    Err(ApiError::Message(format!(
        "予定の削除に失敗（HTTP {}）: {}",
        status.as_u16(),
        body.chars().take(300).collect::<String>()
    )))
}
