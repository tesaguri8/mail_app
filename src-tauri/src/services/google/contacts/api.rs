//! Google People API v1 の薄いラッパー。必要なフィールドだけ受け取る。
//! ドキュメント: https://developers.google.com/people/api/rest/v1/people.connections/list

use serde::Deserialize;

/// API 呼び出しのエラー。増分同期トークンの失効は上位でフル同期に切り替える。
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// syncToken が失効した（410 Gone / EXPIRED_SYNC_TOKEN）。フル同期し直す必要がある。
    #[error("同期トークンが失効しました")]
    SyncTokenExpired,
    /// 送った etag が古い（Google 側が先に更新されている）。読み直してから送り直す。
    #[error("Google 側が先に更新されています（etag 不一致）")]
    EtagConflict,
    /// その他のエラー（メッセージ）。
    #[error("{0}")]
    Message(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(e: reqwest::Error) -> Self {
        ApiError::Message(e.to_string())
    }
}

// ── レスポンス型（読み取り用） ────────────────────────────────────────

/// 値ごとのメタ情報。`primary` が主値、`deleted` は増分同期での削除印。
#[derive(Debug, Deserialize, Default)]
pub struct GFieldMetadata {
    #[serde(default)]
    pub primary: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct GPersonMetadata {
    /// 増分同期で「Google 側から削除された」ことを示す。
    #[serde(default)]
    pub deleted: bool,
}

#[derive(Debug, Deserialize, Default)]
pub struct GName {
    #[serde(rename = "displayName", default)]
    pub display_name: Option<String>,
    #[serde(rename = "familyName", default)]
    pub family_name: Option<String>,
    #[serde(rename = "givenName", default)]
    pub given_name: Option<String>,
    #[serde(rename = "phoneticFamilyName", default)]
    pub phonetic_family_name: Option<String>,
    #[serde(rename = "phoneticGivenName", default)]
    pub phonetic_given_name: Option<String>,
    #[serde(rename = "middleName", default)]
    pub middle_name: Option<String>,
    #[serde(rename = "phoneticMiddleName", default)]
    pub phonetic_middle_name: Option<String>,
    #[serde(rename = "honorificPrefix", default)]
    pub honorific_prefix: Option<String>,
    #[serde(rename = "honorificSuffix", default)]
    pub honorific_suffix: Option<String>,
    #[serde(default)]
    pub metadata: GFieldMetadata,
}

/// ニックネーム。
#[derive(Debug, Deserialize, Default)]
pub struct GNickname {
    #[serde(default)]
    pub value: Option<String>,
}

/// メール・電話・URL に共通の「値＋種別」。
#[derive(Debug, Deserialize, Default)]
pub struct GTypedValue {
    #[serde(default)]
    pub value: Option<String>,
    /// 'home' | 'work' | 'mobile' | 'otherFax' などの機械可読な種別。
    #[serde(rename = "type", default)]
    pub value_type: Option<String>,
    /// 表示用（ロケール依存。カスタム種別はこちらにだけ入る）。
    #[serde(rename = "formattedType", default)]
    pub formatted_type: Option<String>,
    #[serde(default)]
    pub metadata: GFieldMetadata,
}

#[derive(Debug, Deserialize, Default)]
pub struct GAddress {
    #[serde(rename = "type", default)]
    pub value_type: Option<String>,
    #[serde(rename = "formattedType", default)]
    pub formatted_type: Option<String>,
    #[serde(rename = "postalCode", default)]
    pub postal_code: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub city: Option<String>,
    #[serde(rename = "streetAddress", default)]
    pub street_address: Option<String>,
    #[serde(rename = "extendedAddress", default)]
    pub extended_address: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(rename = "countryCode", default)]
    pub country_code: Option<String>,
    #[serde(rename = "poBox", default)]
    pub po_box: Option<String>,
    #[serde(default)]
    pub metadata: GFieldMetadata,
}

#[derive(Debug, Deserialize, Default)]
pub struct GOrganization {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "phoneticName", default)]
    pub phonetic_name: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub metadata: GFieldMetadata,
}

/// 誕生日。年が無いことがある（`date.year` が省略される）。
#[derive(Debug, Deserialize, Default)]
pub struct GDate {
    #[serde(default)]
    pub year: Option<i32>,
    #[serde(default)]
    pub month: Option<u32>,
    #[serde(default)]
    pub day: Option<u32>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GBirthday {
    #[serde(default)]
    pub date: Option<GDate>,
    /// 自由入力（date が無いときの原文）。
    #[serde(default)]
    pub text: Option<String>,
}

/// 記念日などの日付（events）。
#[derive(Debug, Deserialize, Default)]
pub struct GEvent {
    #[serde(default)]
    pub date: Option<GDate>,
    #[serde(rename = "type", default)]
    pub value_type: Option<String>,
    #[serde(rename = "formattedType", default)]
    pub formatted_type: Option<String>,
}

/// 関係（relations）。
#[derive(Debug, Deserialize, Default)]
pub struct GRelation {
    #[serde(default)]
    pub person: Option<String>,
    #[serde(rename = "type", default)]
    pub value_type: Option<String>,
    #[serde(rename = "formattedType", default)]
    pub formatted_type: Option<String>,
}

/// チャット（imClients）。
#[derive(Debug, Deserialize, Default)]
pub struct GImClient {
    #[serde(default)]
    pub username: Option<String>,
    /// 'skype' | 'googleTalk' などの機械可読な名前、またはカスタム名。
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(rename = "formattedProtocol", default)]
    pub formatted_protocol: Option<String>,
    #[serde(rename = "type", default)]
    pub value_type: Option<String>,
    #[serde(rename = "formattedType", default)]
    pub formatted_type: Option<String>,
}

/// カスタム項目（userDefined）。
#[derive(Debug, Deserialize, Default)]
pub struct GUserDefined {
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GBiography {
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GContactGroupMembership {
    #[serde(rename = "contactGroupId", default)]
    pub contact_group_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct GMembership {
    #[serde(rename = "contactGroupMembership", default)]
    pub contact_group_membership: Option<GContactGroupMembership>,
}

/// 連絡先 1 件。
#[derive(Debug, Deserialize, Default)]
pub struct GPerson {
    #[serde(rename = "resourceName", default)]
    pub resource_name: Option<String>,
    /// 更新時に必須（読んだ版の etag を送らないと弾かれる）。
    #[serde(default)]
    pub etag: Option<String>,
    #[serde(default)]
    pub metadata: GPersonMetadata,
    #[serde(default)]
    pub names: Vec<GName>,
    #[serde(default)]
    pub nicknames: Vec<GNickname>,
    #[serde(rename = "emailAddresses", default)]
    pub email_addresses: Vec<GTypedValue>,
    #[serde(rename = "phoneNumbers", default)]
    pub phone_numbers: Vec<GTypedValue>,
    #[serde(default)]
    pub addresses: Vec<GAddress>,
    #[serde(default)]
    pub organizations: Vec<GOrganization>,
    #[serde(default)]
    pub biographies: Vec<GBiography>,
    #[serde(default)]
    pub birthdays: Vec<GBirthday>,
    #[serde(default)]
    pub urls: Vec<GTypedValue>,
    #[serde(default)]
    pub events: Vec<GEvent>,
    #[serde(default)]
    pub relations: Vec<GRelation>,
    #[serde(rename = "imClients", default)]
    pub im_clients: Vec<GImClient>,
    #[serde(rename = "userDefined", default)]
    pub user_defined: Vec<GUserDefined>,
    #[serde(default)]
    pub memberships: Vec<GMembership>,
}

#[derive(Debug, Deserialize, Default)]
pub struct ConnectionsPage {
    #[serde(default)]
    pub connections: Vec<GPerson>,
    #[serde(rename = "nextPageToken", default)]
    pub next_page_token: Option<String>,
    #[serde(rename = "nextSyncToken", default)]
    pub next_sync_token: Option<String>,
}

/// 連絡先グループ（Google の「ラベル」）。
#[derive(Debug, Deserialize, Default)]
pub struct GContactGroup {
    #[serde(rename = "resourceName", default)]
    pub resource_name: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// 'userContactGroup'（ユーザー作成）か 'systemContactGroup'（myContacts 等）。
    #[serde(rename = "groupType", default)]
    pub group_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ContactGroupsPage {
    #[serde(rename = "contactGroups", default)]
    contact_groups: Vec<GContactGroup>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
}

// ── リクエスト ──────────────────────────────────────────────────────

/// HTTP ステータスを見て、失効トークンだけ専用エラーに振り分ける。
async fn check(resp: reqwest::Response) -> Result<reqwest::Response, ApiError> {
    if resp.status().is_success() {
        return Ok(resp);
    }
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    // 失効時は 400 EXPIRED_SYNC_TOKEN で返ることがあり、410 とは限らない。本文でも判定する。
    if status.as_u16() == 410 || body.contains("EXPIRED_SYNC_TOKEN") {
        return Err(ApiError::SyncTokenExpired);
    }
    // 更新は読んだ版の etag を要求する。古いと 400/409/412 で etag に触れた本文が返る。
    if matches!(status.as_u16(), 400 | 409 | 412) && body.contains("etag") {
        return Err(ApiError::EtagConflict);
    }
    Err(ApiError::Message(format!(
        "People API エラー (HTTP {}): {}",
        status.as_u16(),
        body.chars().take(300).collect::<String>()
    )))
}

/// 連絡先を 1 ページ取得する。`sync_token` があれば増分、なければフル。
///
/// フルでも増分でも `requestSyncToken=true` を付け、最終ページで次回用のトークンを受け取る。
/// 増分では削除された連絡先が `metadata.deleted = true` で返る。
pub async fn list_connections(
    client: &reqwest::Client,
    token: &str,
    sync_token: Option<&str>,
    page_token: Option<&str>,
) -> Result<ConnectionsPage, ApiError> {
    let mut query: Vec<(&str, String)> = vec![
        ("personFields", super::PERSON_FIELDS.into()),
        ("pageSize", "1000".into()),
        ("requestSyncToken", "true".into()),
    ];
    if let Some(st) = sync_token {
        query.push(("syncToken", st.into()));
    }
    if let Some(pt) = page_token {
        query.push(("pageToken", pt.into()));
    }
    let resp = client
        .get(format!("{}/people/me/connections", super::API_BASE))
        .bearer_auth(token)
        .query(&query)
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// 連絡先グループ（ラベル）の一覧をページングを畳んで返す。
pub async fn list_contact_groups(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<GContactGroup>, ApiError> {
    let mut out = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
        let mut query: Vec<(&str, String)> = vec![("pageSize", "200".into())];
        if let Some(pt) = &page_token {
            query.push(("pageToken", pt.clone()));
        }
        let resp = client
            .get(format!("{}/contactGroups", super::API_BASE))
            .bearer_auth(token)
            .query(&query)
            .send()
            .await?;
        let page: ContactGroupsPage = check(resp).await?.json().await?;
        out.extend(page.contact_groups);
        match page.next_page_token {
            Some(next) => page_token = Some(next),
            None => break,
        }
    }
    Ok(out)
}

/// 連絡先を 1 件読む（`people.get`）。送信の直前に読み直し、その内容を土台に Rondine が扱う
/// 部分だけを上書きするため、型に落とさず JSON のまま返す（知らない項目も保つため）。
pub async fn get_person(
    client: &reqwest::Client,
    token: &str,
    resource_name: &str,
) -> Result<serde_json::Value, ApiError> {
    let resp = client
        .get(format!("{}/{resource_name}", super::API_BASE))
        .bearer_auth(token)
        .query(&[("personFields", super::PERSON_FIELDS)])
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// 連絡先を 1 件作成する（`people:createContact`）。作成された Person（resourceName / etag つき）を返す。
pub async fn create_contact(
    client: &reqwest::Client,
    token: &str,
    body: &serde_json::Value,
) -> Result<GPerson, ApiError> {
    let resp = client
        .post(format!("{}/people:createContact", super::API_BASE))
        .bearer_auth(token)
        .query(&[("personFields", super::PERSON_FIELDS)])
        .json(body)
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// 連絡先を 1 件更新する（`people/*:updateContact`）。
///
/// `body` には**読んだ版の etag を必ず含める**こと（含めないと People API に弾かれる）。
/// `updatePersonFields` に挙げた項目だけが置き換わり、挙げなかった項目は Google 側で保持される
/// （＝Rondine が扱わない写真などは触らない）。
pub async fn update_contact(
    client: &reqwest::Client,
    token: &str,
    resource_name: &str,
    body: &serde_json::Value,
) -> Result<GPerson, ApiError> {
    let resp = client
        .patch(format!("{}/{resource_name}:updateContact", super::API_BASE))
        .bearer_auth(token)
        .query(&[
            ("updatePersonFields", super::WRITE_PERSON_FIELDS),
            ("personFields", super::PERSON_FIELDS),
        ])
        .json(body)
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// 連絡先を 1 件削除する（`people/*:deleteContact`）。
pub async fn delete_contact(
    client: &reqwest::Client,
    token: &str,
    resource_name: &str,
) -> Result<(), ApiError> {
    let resp = client
        .delete(format!("{}/{resource_name}:deleteContact", super::API_BASE))
        .bearer_auth(token)
        .send()
        .await?;
    check(resp).await?;
    Ok(())
}

/// 連絡先グループ（ラベル）を 1 つ作る（`contactGroups.create`）。
pub async fn create_contact_group(
    client: &reqwest::Client,
    token: &str,
    name: &str,
) -> Result<GContactGroup, ApiError> {
    let resp = client
        .post(format!("{}/contactGroups", super::API_BASE))
        .bearer_auth(token)
        .json(&serde_json::json!({ "contactGroup": { "name": name } }))
        .send()
        .await?;
    Ok(check(resp).await?.json().await?)
}

/// グループの所属を変更する（`contactGroups/*/members:modify`）。
///
/// 連絡先の所属は `people:updateContact` では変えられない（`updatePersonFields` に
/// `memberships` を挙げても通らない）ので、こちらの専用エンドポイントを使う。
/// `group_id` は resourceName の 'contactGroups/' を除いた部分。
pub async fn modify_contact_group_members(
    client: &reqwest::Client,
    token: &str,
    group_id: &str,
    add: &[String],
    remove: &[String],
) -> Result<(), ApiError> {
    if add.is_empty() && remove.is_empty() {
        return Ok(());
    }
    let resp = client
        .post(format!(
            "{}/contactGroups/{group_id}/members:modify",
            super::API_BASE
        ))
        .bearer_auth(token)
        .json(&serde_json::json!({
            "resourceNamesToAdd": add,
            "resourceNamesToRemove": remove,
        }))
        .send()
        .await?;
    check(resp).await?;
    Ok(())
}
