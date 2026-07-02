import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, Check, ChevronDown, Plus } from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { organizationList } from '../services/organizations';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const MAX_SUGGESTIONS = 8;

/**
 * 会社・組織のコンボボックス。入力に応じて組織データベースを検索し、候補から選ぶ
 * オートコンプリート。既存を選べば org_id を確定（照合はID）、一覧に無い名前を
 * 入力すれば新規登録扱い（保存時に組織を作成）。
 */
export function OrgCombobox({
  orgId,
  name,
  onChange,
}: {
  orgId: number | null;
  name: string;
  onChange: (orgId: number | null, name: string) => void;
}) {
  const { t } = useTranslation();
  const [results, setResults] = useState<OrganizationSummary[]>([]);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const boxRef = useRef<HTMLDivElement>(null);

  const query = name.trim();

  // 開いている間、入力に応じて組織DBを検索（軽いデバウンス）。
  useEffect(() => {
    if (!isTauri || !open) return;
    let alive = true;
    const h = setTimeout(() => {
      organizationList(query)
        .then((r) => alive && setResults(r))
        .catch(() => alive && setResults([]));
    }, 120);
    return () => {
      alive = false;
      clearTimeout(h);
    };
  }, [query, open]);

  // 外側クリックで閉じる。
  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, []);

  const visible = results.slice(0, MAX_SUGGESTIONS);
  const exact = results.find((o) => o.name.toLowerCase() === query.toLowerCase()) ?? null;
  const isExisting = orgId != null || exact != null;

  const pick = (o: OrganizationSummary) => {
    onChange(o.id, o.name);
    setOpen(false);
    setHighlight(-1);
  };

  const handleInput = (text: string) => {
    // 現在の候補に完全一致があれば org_id を確定、無ければ null（保存時に find-or-create）。
    const key = text.trim().toLowerCase();
    const match = results.find((o) => o.name.toLowerCase() === key);
    onChange(match ? match.id : null, text);
    setOpen(true);
    setHighlight(-1);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) {
      setOpen(true);
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, visible.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === 'Enter') {
      if (open && highlight >= 0 && visible[highlight]) {
        e.preventDefault();
        pick(visible[highlight]);
      }
    } else if (e.key === 'Escape') {
      setOpen(false);
    }
  };

  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
        <Building2 size={15} />
        {t('contact.organization')}
      </span>
      <div ref={boxRef} className="relative">
        <input
          className="w-full rounded bg-white/10 px-2.5 py-1.5 pr-8 text-sm outline-none focus:bg-white/15"
          value={name}
          placeholder={t('contact.orgPlaceholder')}
          onChange={(e) => handleInput(e.target.value)}
          onFocus={() => setOpen(true)}
          onKeyDown={onKeyDown}
          role="combobox"
          aria-expanded={open}
          aria-autocomplete="list"
        />
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          title={t('org.search')}
          aria-label={t('org.search')}
          className="absolute right-1 top-1/2 flex h-6 w-6 -translate-y-1/2 items-center justify-center rounded text-white/40 hover:text-white/70"
        >
          <ChevronDown size={15} className={open ? 'rotate-180' : ''} />
        </button>

        {open && visible.length > 0 && (
          <ul className="absolute left-0 right-0 top-full z-20 mt-1 max-h-60 overflow-y-auto rounded-md border border-white/15 bg-[#141a2e] py-1 shadow-xl">
            {visible.map((o, i) => (
              <li key={o.id}>
                <button
                  type="button"
                  onMouseDown={(e) => {
                    e.preventDefault();
                    pick(o);
                  }}
                  onMouseEnter={() => setHighlight(i)}
                  className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm ${
                    i === highlight ? 'bg-white/15' : 'hover:bg-white/10'
                  }`}
                >
                  <Building2 size={13} className="shrink-0 text-white/40" />
                  <span className="min-w-0 flex-1 truncate">{o.name}</span>
                  <span className="shrink-0 text-xs text-white/40">
                    {t('org.members', { count: o.member_count })}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>

      {query !== '' && (
        <span
          className={`mt-1 flex items-center gap-1 text-[11px] ${
            isExisting ? 'text-emerald-300' : 'text-sky-300'
          }`}
        >
          {isExisting ? <Check size={12} /> : <Plus size={12} />}
          {isExisting
            ? exact
              ? t('contact.orgExistingN', { count: exact.member_count })
              : t('contact.orgExisting')
            : t('contact.orgNew')}
        </span>
      )}
    </div>
  );
}
