//! Google 連絡先（People API）同期。
//!
//! - `api`    : People API v1 の薄い REST ラッパー。
//! - `incoming`: Google の Person → 連絡先の中身（`models::ContactFields`）。
//! - `outgoing`: 連絡先の中身 → People API の書き込み本文（読み直した Person を土台に上書き）。
//! - `sync`    : 送信（push）→ 取り込み（pull）を束ねる同期エンジン。
//!
//! カレンダーと違い、連絡先は**取り込んだものをそのまま住所録へ入れない**。既存の住所録と
//! 全件重複してしまうため、いったん `contact_identities` に台帳として溜め、照合フェーズで
//! 「既存の誰か」と結び付けるか新規にするかを決める（マイグレーション 0056）。

pub mod api;
pub mod incoming;
pub mod outgoing;
pub mod sync;

/// People API v1 のベース URL。
pub const API_BASE: &str = "https://people.googleapis.com/v1";

/// 取得するフィールド（personFields）。Rondine の連絡先モデル（docs/CONTACT_MODEL.md §1 の
/// Google 列）に対応するもの。写真は後続なので取らない。
pub const PERSON_FIELDS: &str = "names,nicknames,emailAddresses,phoneNumbers,addresses,\
organizations,biographies,birthdays,urls,events,relations,imClients,userDefined,memberships,\
metadata";

/// 送信時に置き換える項目（`updatePersonFields`）。
///
/// **挙げた項目は「本文に無ければ消える」。**送る直前に読み直した Person を土台に、Rondine が
/// 扱う部分だけを上書きして送るので、Rondine が知らない付帯情報は保たれる
/// （`services::google::contacts::outgoing`）。`memberships`（ラベル・スター）は
/// `contactGroups/*/members:modify` で別に送る（updateContact では変えられない）。
/// `metadata` は読み取り専用なので挙げられない。
pub const WRITE_PERSON_FIELDS: &str = "names,nicknames,emailAddresses,phoneNumbers,addresses,\
organizations,biographies,birthdays,urls,events,relations,imClients,userDefined";

/// 「スター付き」のシステムグループ（Rondine のお気に入りと同期する）。
pub const STARRED_GROUP: &str = "starred";
