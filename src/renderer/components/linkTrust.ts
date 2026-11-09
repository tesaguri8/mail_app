// 本文のリンクを左クリックで開いてよいかの判定と、行き先の URL の見せ方（純関数）。
// docs/MAIL_SECURITY.md §2.1。会話のバブルと全文表示（MailBody）で同じ判定を使う。

/** 判定に使う差出人の目印（ThreadMessage / MailDetail の該当欄。Rust 側で同じ基準で求める）。 */
export type LinkTrustInput = {
  /** グリーン（本人一致 or 認定ドメイン）。 */
  is_green: boolean;
  /** 住所録に登録済み。 */
  is_known: boolean;
  /** 住所録のお気に入り（VIP）。 */
  is_vip: boolean;
  /** 自分から送ったことがある（返信歴あり）。 */
  is_replied: boolean;
  /** 自分が送ったメール（送信済み・差出人が自分のアカウント）。 */
  own: boolean;
  /** 認証結果からなりすましの疑いがある。 */
  spoof_suspect: boolean;
};

/**
 * 'open' = 左クリックでそのまま開く / 'guarded' = 左クリックでは開かず、右クリックのメニューから開く。
 * なりすましの疑いがあれば、ほかの目印に当たっていても止める（差出人の欄は偽れるため）。
 */
export type LinkTrust = 'open' | 'guarded';

export function linkTrust(m: LinkTrustInput): LinkTrust {
  if (m.spoof_suspect) return 'guarded';
  return m.is_green || m.is_known || m.is_vip || m.is_replied || m.own ? 'open' : 'guarded';
}

/**
 * メニューの上に出す行き先の URL。長ければ途中を「…」で詰め、ドメインは必ず全部残す
 * （どこへ行くのかがいちばん大事なので）。スキームの `https://` は落とさない（http との区別のため）。
 */
export function displayUrl(url: string, max = 64): string {
  if (url.length <= max) return url;
  const m = /^([a-z][a-z0-9+.-]*:(?:\/\/)?)([^/?#]*)(.*)$/i.exec(url);
  if (!m) return `${url.slice(0, max - 1)}…`;
  const [, scheme, host, rest] = m;
  const head = scheme + host;
  // ドメインだけで上限を超えるなら、ドメインまでは削らずに出す（行き先を偽らせない）。
  if (head.length >= max - 1) return rest ? `${head}…` : head;
  const room = max - head.length - 1;
  return `${head}${rest.slice(0, room)}…`;
}
