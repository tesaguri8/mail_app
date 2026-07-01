use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 境界型の例。ts-rs により `src/bindings/AppInfo.ts` を生成する。
/// 生成: `npm run gen:bindings`（= cargo test --lib export_bindings）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub identifier: String,
}

/// データベースの状態（スキーマバージョン・パス）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DbInfo {
    pub schema_version: i32,
    pub path: String,
}

/// プロバイダ自動判定の結果（docs/ONBOARDING.md）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AutoconfigResult {
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub imap_security: String,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: String,
    pub source: String, // "builtin" | "guess"
    pub note: Option<String>,
}

/// アカウント追加の入力（フロントから受け取る）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AccountInput {
    pub email: String,
    pub display_name: Option<String>,
    /// ログイン用サーバーユーザー名（メールアドレスと別にできる）。未指定なら email を使う。
    pub username: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
}

/// アカウント一覧表示用（資格情報は含めない）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AccountSummary {
    pub id: i32,
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: String,
    pub smtp_host: String,
    pub sync_window: String,
    /// フルデータ（本文＋添付）を保持する期間。これより古いと添付を削除。'all'=常に保持。
    pub full_window: String,
    /// 本文の全文を保持する期間。これより古いと要約保存に落とす。'off'=しない。
    pub body_window: String,
    /// 既定署名の ID（未設定なら None）。
    pub signature_id: Option<i32>,
    pub unread_count: i32,
    pub total_count: i32,
}

/// 署名（差出人ごとに使い回せる本文）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SignatureSummary {
    pub id: i32,
    pub name: String,
    pub body: String,
}

/// メールサーバーアカウント設定（接続＋ログイン）。再利用・紐づけ用。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ServerAccountSummary {
    pub id: i32,
    pub name: Option<String>,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub username: String,
}

/// メール一覧表示用（軽量）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MailSummary {
    pub id: i32,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub date: Option<String>,
    pub preview: String,
    pub is_read: bool,
    /// 添付の有無（旧データ由来のヒント。inline を含む場合がある）。
    pub has_attachments: bool,
    /// 実ファイルの添付行（kind='attachment'）が手元にあるか。フィルタ用。
    pub has_real_attachments: bool,
    pub is_starred: bool,
    pub is_bookmarked: bool,
    /// 付与されているタグの ID 群（表示・絞り込み用）。
    pub tag_ids: Vec<i32>,
}

/// ユーザー定義タグ（プロジェクト等の任意ラベル。docs/FILTERING.md）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct TagSummary {
    pub id: i32,
    pub name: String,
    /// 表示色（CSS カラー文字列。未設定なら None）。
    pub color: Option<String>,
    /// 付与されているメール件数。
    pub count: i32,
}

/// ラベル付きの値（メール・電話）。Apple/Google のラベル付き複数値に対応。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactValue {
    pub id: i32,
    /// 見出し（自宅/職場/携帯/カスタム＝会社名など）。
    pub label: Option<String>,
    pub value: String,
    pub is_primary: bool,
}

/// ラベル付きの構造化住所。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactAddress {
    pub id: i32,
    pub label: Option<String>,
    pub postal: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub street: Option<String>,
    pub extended: Option<String>,
    pub country: Option<String>,
    pub is_primary: bool,
}

/// 連絡先（住所録）。一覧・詳細・編集で共通に使う（連絡先はメールほど大量でないため軽量/詳細を分けない）。
/// メール/電話/住所は子テーブル由来のラベル付き複数値（arrays）。flat な email/phone/address は
/// 主(primary)値の写しで、一覧表示や後方互換のために保持する。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactSummary {
    pub id: i32,
    pub display_name: String,
    /// 姓（構造化名。表示名とは別）。
    pub family_name: Option<String>,
    /// 名。
    pub given_name: Option<String>,
    /// よみ（姓）。
    pub phonetic_family: Option<String>,
    /// よみ（名）。
    pub phonetic_given: Option<String>,
    /// 読み（並び替え用。よみ姓＋よみ名の結合など）。
    pub name_kana: Option<String>,
    /// 主メールアドレス（primary の写し）。
    pub email: Option<String>,
    pub phone: Option<String>,
    pub organization: Option<String>,
    /// 役職。
    pub org_title: Option<String>,
    /// 部署。
    pub org_department: Option<String>,
    /// 主住所の整形文字列（primary の写し。一覧用）。
    pub address: Option<String>,
    /// 誕生日（YYYY-MM-DD 等の文字列。ホーム/ウィジェット通知用）。
    pub birthday: Option<String>,
    pub note: Option<String>,
    /// お気に入り（先頭に固定表示）。
    pub is_favorite: bool,
    /// 取引先の手動フラグ（docs/FILTERING.md）。
    pub is_business: bool,
    /// この相手からのメールで外部画像を許可（docs/MAIL_SECURITY.md）。
    pub allow_remote_images: bool,
    /// ラベル付き複数メール（詳細取得時のみ充填。一覧では空）。
    pub emails: Vec<ContactValue>,
    /// ラベル付き複数電話（同上）。
    pub phones: Vec<ContactValue>,
    /// ラベル付き複数住所（同上）。
    pub addresses: Vec<ContactAddress>,
}

/// 連絡先の作成・更新入力（フロントから受け取る）。`id` が None なら新規作成。
/// 姓/名・よみ姓/よみ名は任意（省略時はフロント旧実装との後方互換）。
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactInput {
    pub id: Option<i32>,
    pub display_name: String,
    /// 姓（構造化名）。
    #[serde(default)]
    pub family_name: Option<String>,
    /// 名。
    #[serde(default)]
    pub given_name: Option<String>,
    /// よみ（姓）。
    #[serde(default)]
    pub phonetic_family: Option<String>,
    /// よみ（名）。
    #[serde(default)]
    pub phonetic_given: Option<String>,
    pub name_kana: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub organization: Option<String>,
    pub address: Option<String>,
    pub birthday: Option<String>,
    pub note: Option<String>,
    pub is_favorite: bool,
    pub is_business: bool,
    pub allow_remote_images: bool,
}

/// 連絡先インポートの結果（vCard 取り込み。docs/IMPORT_EXPORT.md）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ImportReport {
    /// ファイル内の vCard 総数。
    pub total: i32,
    /// 新規追加した件数。
    pub imported: i32,
    /// 既存（UID かメール一致）を更新した件数。
    pub updated: i32,
    /// 連絡先として成立せず飛ばした件数（名前・メール・電話いずれも無い等）。
    pub skipped: i32,
}

/// 重複候補のグループ（整理 UI 用）。record linkage で束ねた連結成分。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DuplicateGroup {
    /// グループの見出し（代表の表示名）。
    pub label: String,
    /// 確信度: "high"（携帯/メール一致）| "medium"（同名＋組織/県）| "low"（同名のみ）。
    pub confidence: String,
    /// 重複候補の連絡先（2 件以上）。
    pub contacts: Vec<ContactSummary>,
}

/// 連絡先グループ（所属件数つき。編集 UI は後続）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct ContactGroupSummary {
    pub id: i32,
    pub name: String,
    pub color: Option<String>,
    /// 所属している連絡先の件数。
    pub count: i32,
}

/// メール詳細（本文表示用）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct MailDetail {
    pub id: i32,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    pub to_addresses: Option<String>,
    pub date: Option<String>,
    pub clean_body: Option<String>,
    pub body_plain: Option<String>,
    /// HTML 本文（あれば）。レンダラ側でテキスト＋リンクのみ安全描画する。
    pub body_html: Option<String>,
    pub has_attachments: bool,
    /// 容量節約のため本文を要約保存に落としてある（clean_body のみ）。全文はサーバー再取得可。
    pub body_compacted: bool,
}

/// 添付ファイル（一覧/ダウンロード状態）。
/// `is_downloaded` が false のときは本体未取得（メタのみ）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AttachmentSummary {
    pub id: i32,
    pub filename: String,
    pub content_type: Option<String>,
    pub size: i32,
    pub is_downloaded: bool,
    /// ダウンロード済みの保存先（未取得なら None）。
    pub file_path: Option<String>,
    /// 'attachment'（本来の添付）| 'inline'（本文埋め込み画像）。
    pub kind: String,
    /// Content-ID（cid: 参照の解決用。山括弧除去済み）。
    pub content_id: Option<String>,
}

/// アカウントのローカル保存容量（添付キャッシュの使用量と上限）。
/// バイト数は f64（TS の number）で扱い、2GB 超でも安全に渡す。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct StorageInfo {
    /// ダウンロード済み添付の合計バイト。
    pub used_bytes: f64,
    /// 上限バイト。
    pub limit_bytes: f64,
}

/// エビクション（添付バイトの追い出し）結果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct EvictionReport {
    /// 追い出した添付の件数。
    pub evicted: i32,
    /// 解放したバイト数。
    pub freed_bytes: f64,
}

/// 保持ポリシー適用（期間ベースの3ティア＋容量上限の保険）の結果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct RetentionReport {
    /// ローカルから削除した添付ファイルの件数（Tier2＋容量保険）。
    pub evicted: i32,
    /// 要約保存に落とした本文の件数（Tier3）。
    pub compacted: i32,
    /// 解放したバイト数（添付＋本文の概算）。
    pub freed_bytes: f64,
}

/// データ保存先（mail.db と添付キャッシュのフォルダ）と使用量。
/// バイト数は f64（TS の number）で大きな値も安全に渡す。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DataLocation {
    /// 現在のデータフォルダ（絶対パス）。
    pub dir: String,
    /// 既定の場所を使っているか（移動していない）。
    pub is_default: bool,
    /// mail.db（＋WAL/SHM）の合計バイト。
    pub db_bytes: f64,
    /// 添付キャッシュの合計バイト。
    pub attachments_bytes: f64,
}

/// 同期結果。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct SyncResult {
    pub fetched: i32,
    pub stored: i32,
    /// 既存メールに uid/添付メタを埋め戻した件数（点検つき再取り込み時に意味を持つ）。
    pub backfilled: i32,
}
