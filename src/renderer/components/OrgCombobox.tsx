import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, Check, ChevronDown, Plus, RotateCcw } from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import { organizationList, organizationRestore } from '../services/organizations';
import { trashRetentionGet } from '../services/trash';
import { trashDaysLeft } from '../utils/trash';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
const MAX_SUGGESTIONS = 8;

/**
 * 組織データベースを入力に応じて検索するオートコンプリート（入力＋候補ドロップダウン）。
 * 業務ロジック（org_id 確定・統合など）は持たず、テキスト編集と候補選択を親へ通知する。
 * onResults は excludeId 除外済みの候補一覧を返す（親が完全一致・統合先の判定に使う）。
 */
export function OrgAutocomplete({
  value,
  onChange,
  onSelect,
  onResults,
  excludeId = null,
  placeholder,
  inputClassName,
  ariaLabel,
}: {
  value: string;
  onChange: (text: string) => void;
  onSelect: (org: OrganizationSummary) => void;
  onResults?: (orgs: OrganizationSummary[]) => void;
  excludeId?: number | null;
  placeholder?: string;
  inputClassName?: string;
  ariaLabel?: string;
}) {
  const { t } = useTranslation();
  const [results, setResults] = useState<OrganizationSummary[]>([]);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const [retention, setRetention] = useState(7);
  const boxRef = useRef<HTMLDivElement>(null);
  const query = value.trim();

  useEffect(() => {
    if (!isTauri) return;
    trashRetentionGet()
      .then(setRetention)
      .catch(() => undefined);
  }, []);

  // 開いている間、入力に応じて組織DBを検索（軽いデバウンス。自分自身は除外。削除済みも含める）。
  useEffect(() => {
    if (!isTauri || !open) return;
    let alive = true;
    const h = setTimeout(() => {
      organizationList(query, true)
        .then((r) => {
          if (!alive) return;
          const filtered = excludeId != null ? r.filter((o) => o.id !== excludeId) : r;
          setResults(filtered);
          onResults?.(filtered);
        })
        .catch(() => alive && setResults([]));
    }, 120);
    return () => {
      alive = false;
      clearTimeout(h);
    };
    // onResults は安定なので除外。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [query, open, excludeId]);

  // 外側クリックで閉じる。
  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, []);

  const visible = results.slice(0, MAX_SUGGESTIONS);

  const pick = (o: OrganizationSummary) => {
    // 削除済み（ゴミ箱）の組織を選んだら復活させてから使う。
    if (o.deleted_at && isTauri) {
      organizationRestore(o.id).catch(() => undefined);
    }
    onSelect({ ...o, deleted_at: null });
    setOpen(false);
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
    <div ref={boxRef} className="relative">
      <input
        className={
          inputClassName ??
          'w-full rounded bg-white/10 px-2.5 py-1.5 pr-8 text-sm outline-none focus:bg-white/15'
        }
        value={value}
        placeholder={placeholder}
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={open}
        aria-autocomplete="list"
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
          setHighlight(-1);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
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
          {visible.map((o, i) => {
            const deleted = o.deleted_at != null;
            return (
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
                  {deleted ? (
                    <RotateCcw size={13} className="shrink-0 text-red-300" />
                  ) : (
                    <Building2 size={13} className="shrink-0 text-white/40" />
                  )}
                  <span className={`min-w-0 flex-1 truncate ${deleted ? 'text-red-300' : ''}`}>
                    {o.name}
                  </span>
                  <span
                    className={`shrink-0 text-xs ${deleted ? 'text-red-300/80' : 'text-white/40'}`}
                  >
                    {deleted
                      ? t('org.trashDaysRestore', {
                          count: trashDaysLeft(o.deleted_at, retention),
                        })
                      : t('org.members', { count: o.member_count })}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

/**
 * 連絡先編集の「会社・組織」欄。組織DB検索のオートコンプリートで、既存を選べば org_id を
 * 確定（照合はID）、一覧に無い名前は新規登録扱い（保存時に組織を作成）。
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
  const query = name.trim();
  const exact = results.find((o) => o.name.toLowerCase() === query.toLowerCase()) ?? null;
  const isExisting = orgId != null || exact != null;

  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-xs text-white/50">
        <Building2 size={15} />
        {t('contact.organization')}
      </span>
      <OrgAutocomplete
        value={name}
        placeholder={t('contact.orgPlaceholder')}
        onResults={setResults}
        onChange={(text) => {
          const m = results.find((o) => o.name.toLowerCase() === text.trim().toLowerCase());
          onChange(m ? m.id : null, text);
        }}
        onSelect={(o) => onChange(o.id, o.name)}
      />
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
