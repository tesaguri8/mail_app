//! 連絡先（住所録）の境界型。docs/CONTACT_MODEL.md §1。
//!
//! 本体（`contacts`）は 1 人 1 行で複数値を持たず、メール・電話・会社などはすべて子テーブル
//! （`position = 0` が主）に置く。境界型も同じ形にそろえ、編集できる中身を [`ContactFields`]
//! にまとめて、一覧/詳細（[`ContactSummary`]）と入力（[`ContactInput`]）の両方で共有する。
//! 取り込み（vCard / Google CSV / Google People API）の中間表現も同じ [`ContactFields`] を使う。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// ラベル付きの値（メール・電話）。並び順の先頭が主値。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactValue {
    /// 見出し（自宅/職場/携帯/FAX/代表/カスタム名）。
    pub label: Option<String>,
    pub value: String,
    /// 複数名で共有する会社の代表値（info@… / 代表電話 / 代表FAX 等）。Rondine 固有の印で、
    /// 人単位の重複判定の手掛かりから除外する。どのサービスにも送らない。
    #[serde(default)]
    pub is_shared: bool,
}

/// 所属する会社（複数可。先頭が主）。組織カードにつながっていれば `org_id` を持つ。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactOrganization {
    /// つながっている組織カード。None＝カードにつながっていない（会社名だけ）。
    pub org_id: Option<i32>,
    /// 会社名（カードにつながっていればカードの名前と同期した写し）。
    pub name: Option<String>,
    /// 会社名のよみ。
    pub phonetic_name: Option<String>,
    /// 役職。
    pub title: Option<String>,
    /// 部署。
    pub department: Option<String>,
}

/// ラベル付きの構造化住所。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactAddress {
    pub label: Option<String>,
    /// 私書箱。
    pub po_box: Option<String>,
    /// 郵便番号。
    pub postal: Option<String>,
    /// 都道府県。
    pub region: Option<String>,
    /// 市区町村。
    pub city: Option<String>,
    /// 番地・建物。
    pub street: Option<String>,
    /// 補足。
    pub extended: Option<String>,
    pub country: Option<String>,
    /// 国コード（ISO 3166-1 alpha-2。iCloud の X-ABADR / Google の countryCode）。
    pub country_code: Option<String>,
}

/// ラベル付きの URL。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactUrl {
    pub label: Option<String>,
    pub value: String,
}

/// 記念日などの日付（誕生日は本体の `birthday`）。`YYYY-MM-DD` か年なしの `--MM-DD`。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactDate {
    pub label: Option<String>,
    pub date: String,
}

/// 関係（配偶者・子・上司など）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactRelation {
    pub label: Option<String>,
    pub name: String,
}

/// ハンドルの種類。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum HandleKind {
    /// インスタントメッセージ（Google の imClients / vCard の IMPP）。
    #[default]
    Im,
    /// SNS のプロフィール（vCard の X-SOCIALPROFILE。Google には無い）。
    Social,
}

impl HandleKind {
    /// DB に保存する綴り（`contact_handles.kind`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Im => "im",
            Self::Social => "social",
        }
    }

    /// DB の綴りから戻す。未知の綴りは IM として扱う。
    pub fn from_db(s: &str) -> Self {
        if s == "social" {
            Self::Social
        } else {
            Self::Im
        }
    }
}

/// チャット・SNS のハンドル。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactHandle {
    pub kind: HandleKind,
    /// サービス名（Skype / LINE / Twitter など）。
    pub service: Option<String>,
    /// ユーザー名またはプロフィールの URL。
    pub value: String,
    pub label: Option<String>,
}

/// カスタム項目（Google の userDefined）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactCustomField {
    pub key: String,
    pub value: String,
}

/// 連絡先がつながっている外部サービス。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "lowercase")]
pub enum ContactProvider {
    Google,
    Icloud,
}

impl ContactProvider {
    /// DB に保存する綴り（`contact_identities.provider`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Icloud => "icloud",
        }
    }

    /// DB の綴りから戻す。未知の綴りは None。
    pub fn from_db(s: &str) -> Option<Self> {
        match s {
            "google" => Some(Self::Google),
            "icloud" => Some(Self::Icloud),
            _ => None,
        }
    }
}

/// つながり（どのサービスのどのアカウントと同期しているか）。一覧・詳細のアイコン用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactLink {
    pub provider: ContactProvider,
    /// 連携アカウントの ID（Google は `google_accounts.id`）。
    pub account_id: i32,
    /// 連携アカウントのメールアドレス（同じサービスに複数アカウントがありうるため添える）。
    pub account_email: Option<String>,
    /// 連携アカウントの名前（設定のカードの呼び名。付けていなければアドレス）。印の表示に使う。
    pub account_label: Option<String>,
    /// 連携アカウントが解除中（つながりは残るが同期されない。再接続すると戻る）。
    pub disconnected: bool,
    /// つながりの状態（作成待ち・削除待ちは次の同期で送る）。
    pub state: ContactLinkState,
}

/// つながりの状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
#[serde(rename_all = "snake_case")]
pub enum ContactLinkState {
    /// 同期中（向こうにある）。
    Synced,
    /// 「このサービスにも保存」を選び、次の同期で作る。
    PendingCreate,
    /// 同期をやめて向こうも消す。次の同期で削除を送る。
    PendingDelete,
}

/// 連絡先の中身（編集できる項目のすべて）。一覧/詳細・入力・取り込みの中間表現で共有する。
///
/// 子テーブル由来の配列は**先頭が主値**（`position = 0`）。一覧（`contact_list`）では
/// 配列とタグは空で返し、詳細（`contact_get`）で充填する。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactFields {
    /// 表示名（vCard の FN）。
    pub display_name: String,
    /// 敬称（N の 4 番目 / Google の honorificPrefix）。
    pub name_prefix: Option<String>,
    /// 姓。
    pub family_name: Option<String>,
    /// ミドルネーム。
    pub middle_name: Option<String>,
    /// 名。
    pub given_name: Option<String>,
    /// 接尾辞（N の 5 番目 / Google の honorificSuffix）。
    pub name_suffix: Option<String>,
    /// よみ（姓）。
    pub phonetic_family: Option<String>,
    /// よみ（ミドル）。
    pub phonetic_middle: Option<String>,
    /// よみ（名）。
    pub phonetic_given: Option<String>,
    /// ニックネーム。
    pub nickname: Option<String>,
    /// 旧姓（iCloud のみ）。
    pub maiden_name: Option<String>,
    /// 誕生日（`YYYY-MM-DD` / 年なし `--MM-DD`）。
    pub birthday: Option<String>,
    pub note: Option<String>,
    /// 会社として表示する（iCloud の X-ABShowAs:COMPANY）。
    #[serde(default)]
    pub show_as_company: bool,
    /// お気に入り（Google の「スター付き」と同期）。
    #[serde(default)]
    pub is_favorite: bool,
    /// 取引先の手動フラグ（Rondine 固有。docs/FILTERING.md）。
    #[serde(default)]
    pub is_business: bool,
    /// この相手からのメールで外部画像を許可（Rondine 固有。docs/MAIL_SECURITY.md）。
    #[serde(default)]
    pub allow_remote_images: bool,
    #[serde(default)]
    pub organizations: Vec<ContactOrganization>,
    #[serde(default)]
    pub emails: Vec<ContactValue>,
    #[serde(default)]
    pub phones: Vec<ContactValue>,
    #[serde(default)]
    pub addresses: Vec<ContactAddress>,
    #[serde(default)]
    pub urls: Vec<ContactUrl>,
    #[serde(default)]
    pub dates: Vec<ContactDate>,
    #[serde(default)]
    pub relations: Vec<ContactRelation>,
    #[serde(default)]
    pub handles: Vec<ContactHandle>,
    #[serde(default)]
    pub custom_fields: Vec<ContactCustomField>,
    /// タグ（メールと共通の tags。Google のラベル / vCard の CATEGORIES）。
    #[serde(default)]
    pub tags: Vec<String>,
}

/// 連絡先（一覧・詳細で共通）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactSummary {
    pub id: i32,
    #[serde(flatten)]
    pub fields: ContactFields,
    /// 並び替え用（よみ優先。保存時に組み立てる）。
    pub sort_name: Option<String>,
    /// アバター画像のパス（同期は後続）。
    pub avatar_path: Option<String>,
    /// 論理削除（ゴミ箱）の日時（UTC 文字列）。非 null＝削除済み。
    pub deleted_at: Option<String>,
    /// 主メール（`contact_emails` の position = 0 の写し。一覧用）。
    pub primary_email: Option<String>,
    /// 主電話（同上）。
    pub primary_phone: Option<String>,
    /// 主の会社名（`contact_organizations` の position = 0 の写し。一覧用）。
    pub primary_organization: Option<String>,
    /// つながっているサービス（無ければ Rondine のみ）。一覧でも充填する。
    pub links: Vec<ContactLink>,
}

/// vCard の版（連絡先の書き出し）。既定は 3.0（Google 連絡先・iPhone・Outlook が読める）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub enum VcardVersion {
    #[default]
    #[serde(rename = "3.0")]
    V3,
    #[serde(rename = "4.0")]
    V4,
}

/// 連絡先の書き出し結果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactExportReport {
    /// 書き出した人数。
    pub exported: i32,
}

/// 郵便番号表から引いた住所 1 件（日本）。住所欄の自動入力に使う。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct PostalAddress {
    /// 郵便番号（「NNN-NNNN」）。
    pub postal: String,
    /// 都道府県。
    pub region: String,
    /// 市区町村。
    pub city: String,
    /// 町域（市区町村の既定の番号なら空）。
    pub town: String,
}

/// 連絡先一覧の 1 行（一覧に出す分だけ）。
///
/// 一覧は数千件になるので、[`ContactSummary`] の全項目（ほとんど空）を IPC に載せると、
/// JSON の組み立てと読み込みだけで画面が止まる。開いたら [`ContactSummary`] を取り直す。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactListItem {
    pub id: i32,
    pub display_name: String,
    /// 主メール（一覧の 2 行目。会社名が無いとき）。
    pub primary_email: Option<String>,
    /// 主の会社名（一覧の 2 行目）。
    pub primary_organization: Option<String>,
    pub is_favorite: bool,
    /// 論理削除（ゴミ箱）の日時（UTC 文字列）。非 null＝削除済み。
    pub deleted_at: Option<String>,
    /// つながっているサービス（同期先の印と、同期先での絞り込みに使う）。
    pub links: Vec<ContactLink>,
}

impl From<ContactSummary> for ContactListItem {
    fn from(c: ContactSummary) -> Self {
        Self {
            id: c.id,
            display_name: c.fields.display_name,
            primary_email: c.primary_email,
            primary_organization: c.primary_organization,
            is_favorite: c.fields.is_favorite,
            deleted_at: c.deleted_at,
            links: c.links,
        }
    }
}

/// 連絡先の作成・更新入力（フロントから受け取る）。`id` が None なら新規作成。
///
/// 配列・タグは**送ったもので置き換える**。画面が扱わない項目も、読み込んだ連絡先の値を
/// そのまま入れて送れば保たれる（[`ContactSummary`] と同じ [`ContactFields`] を持つため）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactInput {
    pub id: Option<i32>,
    #[serde(flatten)]
    pub fields: ContactFields,
}

/// 組織カードになっていない会社名（正規化名でまとめたもの）。組織タブの「整理」用。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct UnlinkedOrgName {
    /// 代表の表記（最も多く使われている表記）。カードにするときの既定名。
    pub name: String,
    /// 同じ正規化名にまとまった表記の一覧（多い順）。
    pub variants: Vec<String>,
    /// この会社名を持つ連絡先の人数（削除済みは除く）。
    pub contact_count: i32,
}

/// 組織カードにつながっていないが、同じ組織らしい人（理由つき）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct OrgLinkCandidate {
    /// 候補の連絡先（一覧用の軽い形）。
    pub contact: ContactSummary,
    /// 会社名が一致した（正規化後）ときの、その人の会社名。
    pub matched_name: Option<String>,
    /// メールのドメインが一致したときの、そのドメイン。
    pub matched_domain: Option<String>,
}

/// 組織カード 1 枚ぶんの「つながっていない人」の候補。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct OrgLinkSuggestion {
    pub org: super::OrganizationSummary,
    pub candidates: Vec<OrgLinkCandidate>,
}

/// 組織の整理（カードにする・つなぐ）の下見。確認欄で知らせるために数える。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct OrgChangeImpact {
    /// 会社名が変わる・会社が足されるため、次の同期でつながっているサービスへ送り直しになる人数。
    pub resent: i32,
}

/// 統合で Google 側から消すことになる連絡先の件数（アカウントごと）。統合の確認画面に出す。
///
/// 統合すると、同じ Google アカウントにつながった ID は 1 つだけ残し、余りは次の同期で
/// Google 側から削除する（docs/CONTACTS_SYNC.md §3-5）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MergeRemoteDeletion {
    /// `google_accounts.id`。
    pub account_id: i32,
    /// アカウントの名前（設定のカードの呼び名。無ければアドレス）。
    pub account_label: String,
    /// 削除する件数（残す 1 件は含まない）。
    pub count: i32,
}

/// まとめて統合する 1 組（確実な重複。下見の一覧用）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SureMergeGroup {
    /// 残す連絡先。
    pub keep_id: i32,
    pub display_name: String,
    /// 組の件数（残す 1 件を含む）。
    pub count: i32,
    /// 見分けるための主なメール・電話（組の中で同じ）。
    pub email: Option<String>,
    pub phone: Option<String>,
}

/// 確実な重複をまとめて統合したらどうなるか（読むだけ）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SureMergePreview {
    pub groups: Vec<SureMergeGroup>,
    /// 組に入っている連絡先の数（統合後は組の数になる）。
    pub contacts: i32,
    /// Google 側から消すことになる件数（アカウントごと）。
    pub remote_deletions: Vec<MergeRemoteDeletion>,
}

/// 確実な重複をまとめて統合した結果。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SureMergeResult {
    /// まとめた組の数（＝残った連絡先の数）。
    pub groups: i32,
    /// 消えた（残す側へまとめた）連絡先の数。
    pub merged: i32,
    /// Google 側から消すために削除待ちにした件数（次の同期で送る）。
    pub remote_deletions: i32,
}
