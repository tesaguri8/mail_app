import type { MailDetail } from '@bindings/MailDetail';

/** 返信の宛先を決めるのに使う、元メールの欄。 */
export type ReplySource = Pick<
  MailDetail,
  'from_address' | 'reply_to' | 'to_addresses' | 'cc_addresses'
>;

/** "名前 <addr>" / "addr" からメールアドレス部分だけを取り出す（小文字化はしない）。 */
export function extractEmail(token: string): string {
  const m = token.match(/<([^>]+)>/);
  return (m ? m[1] : token).trim();
}

/**
 * 複数のアドレスリスト文字列（"名前 <addr>, ..."）を結合し、exclude のメール（自分・差出人など）と
 * 重複を除いて 1 本の文字列にする。表示名付きの表記はそのまま残す。
 */
export function mergeAddressList(lists: string[], exclude: string[]): string {
  const skip = new Set(exclude.map((e) => extractEmail(e).toLowerCase()).filter(Boolean));
  const seen = new Set<string>();
  const out: string[] = [];
  for (const list of lists) {
    for (const token of list.split(',')) {
      const t = token.trim();
      if (!t) continue;
      const email = extractEmail(t).toLowerCase();
      if (!email || skip.has(email) || seen.has(email)) continue;
      seen.add(email);
      out.push(t);
    }
  }
  return out.join(', ');
}

/**
 * 返信・全員返信の宛先（To と Cc）を決める（docs/COMPOSE.md）。
 *
 * - 相手からのメール: To は Reply-To（無ければ From）。全員返信の Cc は「元の To＋元の Cc」から、
 *   自分・差出人・返信先を除いたもの。
 * - 自分が送ったメール（From が自分のアドレスのどれか）: Gmail・Thunderbird と同じく、元の宛先へ
 *   送り直す。To は元の To（自分を除く）、全員返信の Cc は元の Cc（自分と To に入れた相手を除く）。
 *   元の To が自分だけ（自分宛のメモ）なら、自分へ返す。
 *
 * `selfEmails` は自分のアドレス（全アカウント）。大文字小文字は区別しない。
 */
export function replyRecipients(
  s: ReplySource,
  mode: 'reply' | 'replyAll',
  selfEmails: string[]
): { to: string; cc: string } {
  const from = s.from_address ?? '';
  const self = selfEmails.filter((e) => e.trim());
  const selfSet = new Set(self.map((e) => extractEmail(e).toLowerCase()));
  if (selfSet.has(extractEmail(from).toLowerCase())) {
    const to = mergeAddressList([s.to_addresses ?? ''], self);
    if (!to) {
      return { to: from, cc: '' };
    }
    const cc =
      mode === 'replyAll'
        ? mergeAddressList([s.cc_addresses ?? ''], [...self, ...to.split(',')])
        : '';
    return { to, cc };
  }
  // 差出人が Reply-To を指定していればそちらへ返信する（ML・no-reply＋実返信先 等）。
  const replyTarget = s.reply_to?.trim() ? s.reply_to.trim() : from;
  const cc =
    mode === 'replyAll'
      ? mergeAddressList(
          [s.to_addresses ?? '', s.cc_addresses ?? ''],
          [...self, from, ...replyTarget.split(',')]
        )
      : '';
  return { to: replyTarget, cc };
}
