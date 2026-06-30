import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Plus } from 'lucide-react';
import type { TagSummary } from '@bindings/TagSummary';
import type { MailSummary } from '@bindings/MailSummary';
import { DEFAULT_TAG_COLOR } from '../utils/tagColors';

type ApplyState = 'all' | 'some' | 'none';

/**
 * 選択中メールへのタグ付与ポップオーバー（右クリックメニューから開く）。
 * 既存タグのトグルと、新規タグのその場作成に対応。
 */
export function TagPicker({
  x,
  y,
  tags,
  selectedMails,
  onToggle,
  onCreate,
  onClose,
}: {
  x: number;
  y: number;
  tags: TagSummary[];
  selectedMails: MailSummary[];
  /** add=true で選択メール全件に付与、false で全件から解除。 */
  onToggle: (tagId: number, add: boolean) => void;
  /** 新規タグを作成し、選択メールに付与する。 */
  onCreate: (name: string) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  const [draft, setDraft] = useState('');

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const nx = Math.min(x, window.innerWidth - width - 8);
    const ny = Math.min(y, window.innerHeight - height - 8);
    setPos({ x: Math.max(8, nx), y: Math.max(8, ny) });
  }, [x, y]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [onClose]);

  // 選択メール群に対し、各タグが「全件・一部・なし」のどれで付いているか。
  const stateOf = (tagId: number): ApplyState => {
    if (selectedMails.length === 0) return 'none';
    const n = selectedMails.filter((m) => m.tag_ids.includes(tagId)).length;
    return n === 0 ? 'none' : n === selectedMails.length ? 'all' : 'some';
  };

  const create = () => {
    const name = draft.trim();
    if (!name) return;
    onCreate(name);
    setDraft('');
  };

  return (
    <div
      ref={ref}
      style={{ left: pos.x, top: pos.y }}
      className="fixed z-50 max-h-80 w-56 overflow-hidden rounded-md border border-white/15 bg-neutral-900/95 text-sm shadow-xl backdrop-blur"
    >
      <div className="border-b border-white/10 px-3 py-1.5 text-xs text-white/45">
        {t('tag.assign')}
      </div>

      <div className="max-h-44 overflow-y-auto py-1">
        {tags.length === 0 ? (
          <div className="px-3 py-2 text-xs text-white/40">{t('tag.none')}</div>
        ) : (
          tags.map((tag) => {
            const st = stateOf(tag.id);
            return (
              <button
                key={tag.id}
                onClick={() => onToggle(tag.id, st !== 'all')}
                className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-white/85 hover:bg-white/10"
              >
                <span
                  className="flex h-4 w-4 shrink-0 items-center justify-center rounded border"
                  style={{
                    borderColor: tag.color ?? DEFAULT_TAG_COLOR,
                    backgroundColor: st === 'none' ? 'transparent' : (tag.color ?? DEFAULT_TAG_COLOR),
                  }}
                >
                  {st === 'all' && <Check size={11} className="text-white" />}
                  {st === 'some' && <span className="h-0.5 w-2 rounded bg-white" />}
                </span>
                <span className="min-w-0 truncate">{tag.name}</span>
              </button>
            );
          })
        )}
      </div>

      <div className="flex items-center gap-1 border-t border-white/10 p-2">
        <input
          autoFocus
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && create()}
          placeholder={t('tag.newPlaceholder')}
          className="min-w-0 flex-1 rounded bg-white/10 px-2 py-1 text-xs text-white placeholder-white/40 outline-none focus:bg-white/20"
        />
        <button
          onClick={create}
          disabled={!draft.trim()}
          title={t('tag.create')}
          aria-label={t('tag.create')}
          className="flex h-7 w-7 shrink-0 items-center justify-center rounded bg-white/15 text-white/80 hover:bg-white/25 disabled:opacity-40"
        >
          <Plus size={15} />
        </button>
      </div>
    </div>
  );
}
