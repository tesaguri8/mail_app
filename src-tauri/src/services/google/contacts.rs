//! Google 連絡先（People API）同期。
//!
//! - `api`    : People API v1 の薄い REST ラッパー。
//! - `convert`: Google の Person ⇄ 取り込み中間表現（`vcard::ImportedContact`）の変換。
//! - `sync`   : 取り込み（pull）を束ねる同期エンジン。
//!
//! カレンダーと違い、連絡先は**取り込んだものをそのまま住所録へ入れない**。既存の住所録と
//! 全件重複してしまうため、いったん `contact_identities` に台帳として溜め、照合フェーズで
//! 「既存の誰か」と結び付けるか新規にするかを決める（マイグレーション 0054）。

pub mod api;
pub mod convert;
pub mod sync;

/// People API v1 のベース URL。
pub const API_BASE: &str = "https://people.googleapis.com/v1";

/// 取得するフィールド（personFields）。Rondine の連絡先モデルに対応するものだけ要求する。
/// 写真・カスタム項目・関係などは扱わないので取らない（送信時も触らないため保持される）。
pub const PERSON_FIELDS: &str = "names,emailAddresses,phoneNumbers,addresses,organizations,\
biographies,birthdays,memberships,metadata";

/// 送信時に置き換える項目（`updatePersonFields`）。
///
/// **挙げた項目は「本文に無ければ消える」**ので、Rondine が持っている項目だけを挙げる。
/// `memberships`（ラベル）は挙げない — ラベル同期は後続の段で、いま送ると Google 側の
/// ラベル分けを消してしまう。`metadata` は読み取り専用なので挙げられない。
pub const WRITE_PERSON_FIELDS: &str =
    "names,emailAddresses,phoneNumbers,addresses,organizations,biographies,birthdays";
