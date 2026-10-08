// 連絡先のラベル（見出し）の候補。保存される値そのものなので翻訳はしない。
//
// 語彙は Rust 側の services::contact_labels（取り込み・Google への送信で使う表）に揃える。
// ここに無い名前も自由に入力でき、その場合はカスタム名としてそのまま保存・送信される。

/** メール・電話・住所のラベル。 */
export const VALUE_LABELS = ['自宅', '職場', '携帯', 'FAX', '代表', 'ポケベル'] as const;

/** URL のラベル。 */
export const URL_LABELS = ['ホームページ', 'ブログ', 'プロフィール', '自宅', '職場'] as const;

/** 日付（記念日など）のラベル。 */
export const DATE_LABELS = ['記念日'] as const;

/** 関係のラベル。 */
export const RELATION_LABELS = [
  '配偶者',
  'パートナー',
  '同居人',
  '子',
  '母',
  '父',
  '親',
  '兄弟',
  '姉妹',
  '親戚',
  '友人',
  '上司',
  'アシスタント',
  '紹介者',
] as const;

/** チャット（IM）のサービス名の候補。Google の imClients の protocol に当たる綴りを含む。 */
export const IM_SERVICES = [
  'Skype',
  'LINE',
  'Jabber',
  'QQ',
  'ICQ',
  'Yahoo',
  'AIM',
  'MSN',
  'googleTalk',
] as const;

/** SNS のサービス名の候補（iCloud の X-SOCIALPROFILE。Google には送られない）。 */
export const SOCIAL_SERVICES = [
  'Twitter',
  'Facebook',
  'Instagram',
  'LinkedIn',
  'Mastodon',
  'GitHub',
] as const;

/** 候補の datalist の id（編集画面に 1 度だけ置き、各行の input の list から参照する）。 */
export const LABEL_LIST_IDS = {
  value: 'contact-label-options',
  url: 'contact-url-label-options',
  date: 'contact-date-label-options',
  relation: 'contact-relation-label-options',
  im: 'contact-im-service-options',
  social: 'contact-social-service-options',
} as const;

export type LabelListKind = keyof typeof LABEL_LIST_IDS;

/** datalist の id と候補の対応。 */
export const LABEL_LISTS: Record<LabelListKind, readonly string[]> = {
  value: VALUE_LABELS,
  url: URL_LABELS,
  date: DATE_LABELS,
  relation: RELATION_LABELS,
  im: IM_SERVICES,
  social: SOCIAL_SERVICES,
};
