import { useTranslation } from 'react-i18next';
import { AlertTriangle } from 'lucide-react';

/**
 * 画面内の確認ダイアログ（取り消せない操作の前に出す）。
 *
 * window.confirm は Linux の WebView で素通りする（`[実測]`）ので、破壊的な操作の確認は
 * これで行う。`notes` は本文の下に並べる補足（「Google の連絡先からも削除されます」など）。
 */
export function ConfirmDialog({
  title,
  body,
  notes = [],
  confirmLabel,
  danger = false,
  busy = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  body: string;
  notes?: string[];
  confirmLabel: string;
  /** 削除など取り消せない操作（実行ボタンを赤にする）。 */
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={onCancel}
    >
      <div
        role="alertdialog"
        aria-modal="true"
        aria-label={title}
        className="w-full max-w-md rounded-lg border border-white/15 bg-[#141a2e] p-5 shadow-xl"
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className={`mb-2 flex items-center gap-2 ${danger ? 'text-red-200' : 'text-amber-200'}`}
        >
          <AlertTriangle size={18} />
          <h3 className="text-base font-semibold">{title}</h3>
        </div>
        <p className="text-sm text-white/70">{body}</p>
        {notes.length > 0 && (
          <ul className="mt-2 space-y-1">
            {notes.map((n) => (
              <li
                key={n}
                className="rounded-md bg-amber-300/10 px-2.5 py-1.5 text-xs text-amber-100"
              >
                {n}
              </li>
            ))}
          </ul>
        )}
        <div className="mt-4 flex justify-end gap-2">
          <button
            onClick={onCancel}
            disabled={busy}
            // 既定のフォーカスは「キャンセル」（Enter で消してしまわないように）。
            autoFocus
            className="rounded-md border border-white/20 px-3 py-1.5 text-sm text-white/70 hover:bg-white/10 disabled:opacity-40"
          >
            {t('org.cancel')}
          </button>
          <button
            onClick={onConfirm}
            disabled={busy}
            className={`rounded-md px-3 py-1.5 text-sm font-medium text-white disabled:opacity-40 ${
              danger ? 'bg-red-500/80 hover:bg-red-500' : 'bg-emerald-500/80 hover:bg-emerald-500'
            }`}
          >
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}
