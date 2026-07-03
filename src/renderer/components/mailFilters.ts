import { Flag, Gem, LeafyGreen, Mail, Paperclip, Star, UserRound, type LucideIcon } from 'lucide-react';
import type { MailSummary } from '@bindings/MailSummary';

/** メール一覧の絞り込みトグル（メール画面・ホームの展開で共用）。
 *  flag（要再確認）は設定手段が入るまで非適用（アイコンのみ）。 */
export const MAIL_FILTERS: { key: string; Icon: LucideIcon }[] = [
  { key: 'unread', Icon: Mail },
  { key: 'star', Icon: Star },
  { key: 'green', Icon: LeafyGreen },
  { key: 'vip', Icon: Gem },
  { key: 'known', Icon: UserRound },
  { key: 'attachment', Icon: Paperclip },
  { key: 'flag', Icon: Flag },
];

/** 選択中のトグルにメールが一致するか（AND）。 */
export function matchesFilters(m: MailSummary, filters: Set<string>): boolean {
  if (filters.has('unread') && m.is_read) return false;
  if (filters.has('attachment') && !m.has_real_attachments) return false;
  if (filters.has('star') && !m.is_starred) return false;
  if (filters.has('green') && !m.is_green) return false; // 差出人がグリーン（認定ドメイン/本人）
  if (filters.has('vip') && !m.is_vip) return false; // 差出人が住所録のお気に入り(Gem)
  if (filters.has('known') && !m.is_known) return false; // 差出人が住所録に登録済み
  // flag（要再確認）はマーク手段が入るまでフィルタしない（空表示で混乱させない）
  return true;
}
