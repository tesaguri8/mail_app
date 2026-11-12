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
import type { ThreadFilter } from '@bindings/ThreadFilter';
import type { ThreadToggle } from '@bindings/ThreadToggle';

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

/** DB で絞れるトグル（キー → ThreadToggle）。flag は非適用なので載せない。 */
const DB_TOGGLES: Record<string, ThreadToggle> = {
  unread: 'unread',
  attachment: 'attachment',
  star: 'star',
  green: 'green',
  vip: 'vip',
  known: 'known',
  replied: 'replied',
};

/**
 * 画面の絞り込み（トグル・反転・期間の境目・タグ）を、一覧と日ごとの件数の問い合わせに渡す形にする
 * （docs/MAIL_LIST_TIMELINE.md §3）。matchesFilters / matchesNoneOfFilters / matchesDate / matchesTags と
 * 同じ結果を DB で出す。何も絞らなければ null（flag だけを選んだときも、今までどおり絞らない）。
 */
export function toThreadFilter(
  filters: Set<string>,
  invert: boolean,
  date: ThreadFilter['date'],
  tagIds: Set<number>
): ThreadFilter | null {
  const toggles = [...filters].flatMap((k) => (k in DB_TOGGLES ? [DB_TOGGLES[k]] : []));
  if (toggles.length === 0 && date == null && tagIds.size === 0) return null;
  // 反転は「選んだ条件のどれにも当てはまらない」。条件が無ければ反転は効かない（画面と同じ）。
  return { toggles, invert: invert && toggles.length > 0, date, tag_ids: [...tagIds] };
}

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

/**
 * 差出人の表示名の色分け（一覧の行）。返信したことがある相手（`is_replied`）がいちばん近い相手で、
 * 住所録にいるだけの相手（`is_known`）がその次。どちらでもなければ null（いまのまま）。
 * どちらも**差出人のアドレス**で判定した値なので、宛先を表示する行（送信済み・下書き。outgoing）には付けない
 * （自分の差出人で判定した色が、宛先の名前に付いて見えてしまう）。
 */
export function senderTone(
  m: Pick<FilterableRow, 'is_known' | 'is_replied'>,
  outgoing: boolean
): 'replied' | 'known' | null {
  if (outgoing) return null;
  if (m.is_replied) return 'replied';
  if (m.is_known) return 'known';
  return null;
}
