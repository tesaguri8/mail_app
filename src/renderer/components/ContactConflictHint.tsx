import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { AlertTriangle, ExternalLink, Users } from 'lucide-react';

/** 値が一致した既存の連絡先（名前と、開くための id）。 */
export interface ConflictOwner {
  id: number;
  display_name: string;
}

/** 説明の中に名前で並べる上限（それより多いときは「ほか N 件」）。 */
const MAX_OWNERS = 3;

/**
 * 既存の別の連絡先と値が一致しているときの ⚠。ホバーで理由（「〇〇」と同じ番号です）を出し、
 * 押すと小さな説明が開く。説明からはその連絡先を開ける。`onMakeShared` があれば
 * 「共有の値にする」も出す（共有の値は重複の手掛かりにしないので、警告が消える）。
 */
export function ContactConflictHint({
  owners,
  kind,
  onOpenContact,
  onMakeShared,
}: {
  owners: readonly ConflictOwner[];
  kind: 'email' | 'phone';
  onOpenContact?: (id: number) => void;
  onMakeShared?: () => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  // 外側を押す・Esc で閉じる。
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const reasonKey = kind === 'email' ? 'contact.dupWithEmail' : 'contact.dupWithPhone';
  const shown = owners.slice(0, MAX_OWNERS);
  const rest = owners.length - shown.length;
  const tooltip = [
    ...shown.map((o) => t(reasonKey, { name: o.display_name })),
    ...(rest > 0 ? [t('contact.dupWithMore', { count: rest })] : []),
  ].join('\n');

  return (
    <div ref={boxRef} className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={tooltip}
        aria-label={tooltip}
        aria-expanded={open}
        className="flex h-7 w-7 items-center justify-center rounded-full text-red-300 hover:bg-red-500/15"
      >
        <AlertTriangle size={14} />
      </button>
      {open && (
        <div
          role="dialog"
          className="absolute right-0 top-full z-30 mt-1 w-72 rounded-md border border-white/15 bg-neutral-900/95 p-3 text-xs text-white/80 shadow-lg backdrop-blur"
        >
          <ul className="space-y-1">
            {shown.map((o) => (
              <li key={o.id} className="flex items-center gap-2">
                <span className="min-w-0 flex-1">{t(reasonKey, { name: o.display_name })}</span>
                {onOpenContact && (
                  <button
                    type="button"
                    onClick={() => {
                      setOpen(false);
                      onOpenContact(o.id);
                    }}
                    className="flex shrink-0 items-center gap-1 rounded px-1.5 py-0.5 text-sky-300 hover:bg-white/10 hover:text-sky-200"
                  >
                    <ExternalLink size={12} />
                    {t('contact.dupOpenContact')}
                  </button>
                )}
              </li>
            ))}
            {rest > 0 && (
              <li className="text-white/50">{t('contact.dupWithMore', { count: rest })}</li>
            )}
          </ul>
          {onMakeShared && (
            <div className="mt-2 border-t border-white/10 pt-2">
              <p className="text-white/55">{t('contact.dupSharedHint')}</p>
              <button
                type="button"
                onClick={() => {
                  setOpen(false);
                  onMakeShared();
                }}
                className="mt-1.5 flex items-center gap-1 rounded bg-amber-400/20 px-2 py-1 text-amber-100 hover:bg-amber-400/30"
              >
                <Users size={12} />
                {t('contact.dupMakeShared')}
              </button>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
