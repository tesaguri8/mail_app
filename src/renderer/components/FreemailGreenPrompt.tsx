import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { LeafyGreen } from 'lucide-react';
import {
  createSenderContact,
  findSenderInBook,
  restoreSenderContact,
  senderDisplayName,
  type SenderInBook,
} from '../utils/senderContact';

/** グリーンの確認・お知らせの枠（狭い幅でも折り返す。SenderGreen と共有）。 */
export const GREEN_NOTICE_BOX =
  'mt-2 flex min-w-0 flex-wrap items-start gap-x-3 gap-y-2 rounded-md border border-emerald-400/25 bg-emerald-950/40 px-3 py-2 text-xs text-white/80';
/** 枠の中の文言（切らずに折り返す）。 */
export const GREEN_NOTICE_TEXT = 'min-w-0 flex-1 basis-48 whitespace-normal break-words';
/** 主の操作・閉じるボタン。 */
export const GREEN_NOTICE_PRIMARY =
  'rounded bg-emerald-600/80 px-2.5 py-1 text-white hover:bg-emerald-500 disabled:opacity-50';
export const GREEN_NOTICE_SECONDARY =
  'rounded bg-white/10 px-2.5 py-1 text-white/80 hover:bg-white/15 disabled:opacity-50';

/** 確認の中身（読み込み中・確認・処理中・結果）。 */
type Phase =
  | { kind: 'checking' }
  | { kind: 'ask'; book: SenderInBook }
  | { kind: 'busy' }
  | { kind: 'done'; message: string }
  | { kind: 'error'; message: string };

/**
 * フリーメール（gmail.com 等）の差出人で葉っぱを押して断られたときの確認
 * （docs/GREEN_DOMAINS.md）。ドメインごとには認めず、差出人を住所録に登録して「この人だけ」
 * グリーンにするかを訊く。同じアドレスが住所録（ゴミ箱を含む）にあれば作らない
 * （ゴミ箱にだけあれば、戻すかを訊く）。
 *
 * 狭い幅でも全文が読めるよう、文言は折り返して出す（道具の行に詰めない）。
 */
export function FreemailGreenPrompt({
  domain,
  name,
  address,
  onGreen,
  onClose,
}: {
  domain: string;
  /** 差出人の表示名（無ければアドレスで登録する）。 */
  name: string | null;
  address: string;
  /** 住所録に入った（＝この人はグリーン）。親はバッジを変え、一覧を読み直す。 */
  onGreen: () => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<Phase>({ kind: 'checking' });

  useEffect(() => {
    let alive = true;
    setPhase({ kind: 'checking' });
    findSenderInBook(address)
      .then((book) => alive && setPhase({ kind: 'ask', book }))
      .catch((e) => alive && setPhase({ kind: 'error', message: String(e) }));
    return () => {
      alive = false;
    };
  }, [address]);

  const display = senderDisplayName(name, address);
  const run = async (f: () => Promise<unknown>, doneKey: string, doneName: string) => {
    setPhase({ kind: 'busy' });
    try {
      await f();
      setPhase({ kind: 'done', message: t(doneKey, { name: doneName, address }) });
      onGreen();
    } catch (e) {
      setPhase({ kind: 'error', message: t('green.freemailFailed', { error: String(e) }) });
    }
  };

  let message = '';
  let action: { label: string; onClick: () => void } | null = null;
  if (phase.kind === 'ask') {
    const { book } = phase;
    if (book.kind === 'none') {
      message =
        display === address.trim()
          ? t('green.freemailAskNoName', { domain, address })
          : t('green.freemailAsk', { domain, name: display, address });
      action = {
        label: t('green.freemailRegister'),
        onClick: () =>
          void run(() => createSenderContact(name, address), 'green.freemailRegistered', display),
      };
    } else if (book.kind === 'trashed') {
      const n = book.contact.display_name;
      message = t('green.freemailTrashed', { domain, name: n, address });
      action = {
        label: t('green.freemailRestore'),
        onClick: () =>
          void run(() => restoreSenderContact(book.contact), 'green.freemailRestored', n),
      };
    } else {
      message = t('green.freemailAlive', { name: book.contact.display_name, address });
    }
  } else if (phase.kind === 'done' || phase.kind === 'error') {
    message = phase.message;
  }

  const pending = phase.kind === 'checking' || phase.kind === 'busy';
  const finished = phase.kind === 'done' || (phase.kind === 'ask' && !action);
  return (
    <div
      role="alertdialog"
      aria-live="polite"
      data-testid="freemail-green-prompt"
      className={GREEN_NOTICE_BOX}
    >
      <LeafyGreen size={14} className="mt-0.5 shrink-0 text-emerald-300" aria-hidden />
      {/* 長い文言・長いアドレスでも切らずに折り返す（break-words で途中改行も許す）。 */}
      <p className={GREEN_NOTICE_TEXT}>{pending ? t('green.freemailChecking') : message}</p>
      <div className="ml-auto flex shrink-0 items-center gap-2">
        {action && (
          <button onClick={action.onClick} disabled={pending} className={GREEN_NOTICE_PRIMARY}>
            {action.label}
          </button>
        )}
        <button
          onClick={onClose}
          disabled={phase.kind === 'busy'}
          className={GREEN_NOTICE_SECONDARY}
        >
          {finished || phase.kind === 'error'
            ? t('green.freemailClose')
            : t('green.freemailCancel')}
        </button>
      </div>
    </div>
  );
}
