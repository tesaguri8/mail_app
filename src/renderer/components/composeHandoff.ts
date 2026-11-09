// 別ウィンドウへ移した書きかけ（ComposeHandoff）から、作成画面の対象を組み立て直す（純関数）。
// 別ウィンドウは props を受け取れないので、元メールと下書きは id から読み直してここへ渡す。
import type { ComposeHandoff } from '@bindings/ComposeHandoff';
import type { DraftContent } from '@bindings/DraftContent';
import type { MailDetail } from '@bindings/MailDetail';
import type { ComposeTarget } from './Compose';

/**
 * 対象を組み立てる。返信・転送で元メールが読めない、下書きの再編集で下書きが読めない
 * ときは null（引用や In-Reply-To を組めないので、作成画面を出さない）。
 */
export function handoffTarget(
  h: ComposeHandoff,
  source: MailDetail | null,
  draft: DraftContent | null
): ComposeTarget | null {
  switch (h.mode) {
    case 'new':
      return { mode: 'new' };
    case 'draft':
      return draft ? { mode: 'draft', draft, source: source ?? undefined } : null;
    default:
      return source ? { mode: h.mode, source } : null;
  }
}
