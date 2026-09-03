import {
  Flag,
  Gem,
  LeafyGreen,
  Mail,
  Paperclip,
  Reply,
  Star,
  UserRound,
  type LucideIcon,
} from 'lucide-react';
import type { MailSummary } from '@bindings/MailSummary';

/** メール一覧の絞り込みトグル（メール画面・ホームの展開で共用）。
 *  flag（要再確認）は設定手段が入るまで非適用（アイコンのみ）。 */
export const MAIL_FILTERS: { key: string; Icon: LucideIcon }[] = [
  { key: 'unread', Icon: Mail },
  { key: 'star', Icon: Star },
  { key: 'green', Icon: LeafyGreen },
  { key: 'vip', Icon: Gem },
  { key: 'known', Icon: UserRound },
  { key: 'replied', Icon: Reply },
  { key: 'attachment', Icon: Paperclip },
  { key: 'flag', Icon: Flag },
];

/** matchesFilters が参照する最小フィールド（一覧の行＝メール/スレッドどちらでも可）。 */
type FilterableRow = Pick<
  MailSummary,
  | 'is_read'
  | 'has_real_attachments'
  | 'is_starred'
  | 'is_green'
  | 'is_vip'
  | 'is_known'
  | 'is_replied'
>;

/** 選択中のトグルにメールが一致するか（AND）。 */
export function matchesFilters(m: FilterableRow, filters: Set<string>): boolean {
  if (filters.has('unread') && m.is_read) return false;
  if (filters.has('attachment') && !m.has_real_attachments) return false;
  if (filters.has('star') && !m.is_starred) return false;
  if (filters.has('green') && !m.is_green) return false; // 差出人がグリーン（認定ドメイン/本人）
  if (filters.has('vip') && !m.is_vip) return false; // 差出人が住所録のお気に入り(Gem)
  if (filters.has('known') && !m.is_known) return false; // 差出人が住所録に登録済み
  if (filters.has('replied') && !m.is_replied) return false; // 差出人に自分から送ったことがある
  // flag（要再確認）はマーク手段が入るまでフィルタしない（空表示で混乱させない）
  return true;
}

/**
 * 選択中のトグルの **どれにも当てはまらない** か（各条件を否定して AND）。
 * 反転（除外）ボタンの述語。
 *
 * `!matchesFilters()` ではないことに注意。matchesFilters は AND なので、その否定は
 * ド・モルガンで OR になり（「スター**または**知り合いでない」）、条件を足すほど対象が
 * 増えてしまう。反転の用途は「不要メールを一気に絞って一括選択する」ことなので、
 * 求めるのは「スターでもなく、知り合いでもない」＝ 各条件の否定の AND。
 */
export function matchesNoneOfFilters(m: FilterableRow, filters: Set<string>): boolean {
  if (filters.has('unread') && !m.is_read) return false; // 「未読」の否定＝既読だけ
  if (filters.has('attachment') && m.has_real_attachments) return false;
  if (filters.has('star') && m.is_starred) return false;
  if (filters.has('green') && m.is_green) return false;
  if (filters.has('vip') && m.is_vip) return false;
  if (filters.has('known') && m.is_known) return false;
  if (filters.has('replied') && m.is_replied) return false;
  // flag は matchesFilters と同じく非適用（反転でも対象にしない）
  return true;
}
