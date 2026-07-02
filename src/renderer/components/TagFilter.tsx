import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Tag } from 'lucide-react';
import type { TagSummary } from '@bindings/TagSummary';
import { DEFAULT_TAG_COLOR } from '../utils/tagColors';

/** メールのタグが絞り込み条件に合致するか（選択タグのいずれかを含めば真／OR）。 */
export function matchesTags(tagIds: number[], filter: Set<number>): boolean {
  if (filter.size === 0) return true;
  return tagIds.some((id) => filter.has(id));
}

/**
 * タグ絞り込み（タグアイコン＋ポップオーバー）。選択したタグのいずれかを持つメールに絞る。
 */
export function TagFilter({
  tags,
  value,
  onChange,
}: {
  tags: TagSummary[];
  value: Set<number>;
  onChange: (v: Set<number>) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  const on = value.size > 0;
  const toggle = (id: number) => {
    const next = new Set(value);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    onChange(next);
  };
  // 選択済みはツールバー下のチップ行（×で解除）に出すため、リストには未選択のみ表示する。
  const available = tags.filter((tag) => !value.has(tag.id));

  return (
    <div ref={ref} className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        title={t('tag.filter')}
        aria-label={t('tag.filter')}
        aria-pressed={on}
        className={`flex h-8 w-8 items-center justify-center rounded-md ${
          on
            ? 'bg-sky-500/30 text-sky-200 ring-1 ring-sky-300/40'
            : 'text-white/55 hover:text-white/80'
        }`}
      >
        <Tag size={15} />
      </button>

      {/* アイコンの左下を起点に展開（コンテンツ側への重なりは許容。親の overflow-hidden は外してある） */}
      {open && (
        <div className="absolute left-0 top-full z-30 mt-1 max-h-72 w-52 overflow-y-auto rounded-md border border-white/15 bg-neutral-900/65 p-1 shadow-xl backdrop-blur">
          {tags.length === 0 ? (
            <div className="px-3 py-2 text-xs text-white/40">{t('tag.none')}</div>
          ) : (
            <>
              {available.map((tag) => (
                <button
                  key={tag.id}
                  onClick={() => toggle(tag.id)}
                  className="flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-sm text-white/75 hover:bg-white/10"
                >
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    style={{ backgroundColor: tag.color ?? DEFAULT_TAG_COLOR }}
                  />
                  <span className="min-w-0 flex-1 truncate">{tag.name}</span>
                  <span className="shrink-0 text-[10px] text-white/35">{tag.count}</span>
                </button>
              ))}
              {available.length === 0 && (
                <div className="px-3 py-2 text-xs text-white/40">{t('tag.allSelected')}</div>
              )}
              {on && (
                <button
                  onClick={() => onChange(new Set())}
                  className="mt-1 block w-full rounded px-2 py-1 text-left text-xs text-white/55 hover:text-white/80"
                >
                  {t('date.clear')}
                </button>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
