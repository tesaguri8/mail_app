import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronsUpDown } from 'lucide-react';

export type FolderOption = { key: string; label: string; group?: boolean };

/** 標準フォルダ。フィルタリンググループは groups で後から渡す。 */
export const STANDARD_FOLDERS = ['inbox', 'drafts', 'sent', 'trash', 'spam'] as const;

/**
 * フォルダ/グループ選択のコンボボックス（入力オートコンプリート＋ドロップダウン）。
 * 受信箱・下書き・送信済・ごみ箱・スパム＋フィルタリンググループ（複数）から選ぶ。
 */
export function FolderCombobox({
  value,
  onChange,
  groups = [],
}: {
  value: string;
  onChange: (key: string) => void;
  groups?: FolderOption[];
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState('');
  const ref = useRef<HTMLDivElement>(null);

  const options: FolderOption[] = [
    ...STANDARD_FOLDERS.map((k) => ({ key: k, label: t(`mailbox.f_${k}`) })),
    ...groups.map((g) => ({ ...g, group: true })),
  ];
  const current = options.find((o) => o.key === value);

  // 外側クリックで閉じる
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  const q = query.trim().toLowerCase();
  const filtered = q ? options.filter((o) => o.label.toLowerCase().includes(q)) : options;
  const folders = filtered.filter((o) => !o.group);
  const groupItems = filtered.filter((o) => o.group);

  const pick = (k: string) => {
    onChange(k);
    setOpen(false);
    setQuery('');
  };

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        className="flex min-w-[140px] items-center justify-between gap-2 rounded-md bg-white/10 px-2 py-1 text-xs hover:bg-white/15"
      >
        <span className="truncate">{current?.label ?? t('mailbox.f_inbox')}</span>
        <ChevronsUpDown size={13} className="shrink-0 text-white/45" />
      </button>

      {open && (
        <div className="absolute left-0 top-full z-20 mt-1 w-56 overflow-hidden rounded-md border border-white/15 bg-neutral-900/95 shadow-xl backdrop-blur">
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t('mailbox.folderPlaceholder')}
            className="w-full border-b border-white/10 bg-transparent px-3 py-2 text-xs outline-none placeholder-white/40"
          />
          <ul className="max-h-64 overflow-y-auto py-1">
            {folders.map((o) => (
              <li key={o.key}>
                <button
                  onClick={() => pick(o.key)}
                  className={`block w-full px-3 py-1.5 text-left text-xs hover:bg-white/10 ${
                    o.key === value ? 'text-sky-300' : 'text-white/85'
                  }`}
                >
                  {o.label}
                </button>
              </li>
            ))}
            {groupItems.length > 0 && (
              <li className="mt-1 border-t border-white/10 px-3 pb-1 pt-1.5 text-[10px] uppercase tracking-wide text-white/40">
                {t('mailbox.folderGroups')}
              </li>
            )}
            {groupItems.map((o) => (
              <li key={o.key}>
                <button
                  onClick={() => pick(o.key)}
                  className={`block w-full px-3 py-1.5 text-left text-xs hover:bg-white/10 ${
                    o.key === value ? 'text-sky-300' : 'text-white/85'
                  }`}
                >
                  {o.label}
                </button>
              </li>
            ))}
            {filtered.length === 0 && (
              <li className="px-3 py-2 text-xs text-white/40">—</li>
            )}
          </ul>
        </div>
      )}
    </div>
  );
}
