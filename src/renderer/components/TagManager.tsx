import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Minus, Plus } from 'lucide-react';
import type { TagSummary } from '@bindings/TagSummary';
import { tagCreate, tagDelete, tagList, tagSetParent, tagUpdate } from '../services/tags';

/** 親子（parent_id）で並べ、各行に階層の深さを付ける（フォルダ整理のツリー表示用）。 */
function orderTree(items: TagSummary[]): { tag: TagSummary; depth: number }[] {
  const ids = new Set(items.map((t) => t.id));
  const byParent = new Map<number | null, TagSummary[]>();
  for (const t of items) {
    const p = t.parent_id != null && ids.has(t.parent_id) ? t.parent_id : null;
    if (!byParent.has(p)) byParent.set(p, []);
    byParent.get(p)!.push(t);
  }
  const out: { tag: TagSummary; depth: number }[] = [];
  const walk = (parent: number | null, depth: number) => {
    for (const t of byParent.get(parent) ?? []) {
      out.push({ tag: t, depth });
      walk(t.id, depth + 1);
    }
  };
  walk(null, 0);
  return out;
}

/** cand が ancestor の子孫か（親を辿って判定）。循環選択の除外用。 */
function isDescendant(items: TagSummary[], cand: number, ancestor: number): boolean {
  const parentOf = new Map(items.map((t) => [t.id, t.parent_id ?? null]));
  let cur: number | null | undefined = cand;
  while (cur != null) {
    if (cur === ancestor) return true;
    cur = parentOf.get(cur) ?? null;
  }
  return false;
}
import { DEFAULT_TAG_COLOR, pickTagColor, TAG_PALETTE } from '../utils/tagColors';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * タグの管理（一覧・作成・改名・配色・削除）。メール一覧のタグ付け／絞り込みの元データ。
 */
export function TagManager() {
  const { t } = useTranslation();
  const [items, setItems] = useState<TagSummary[]>([]);

  const load = () => {
    if (!isTauri) return;
    tagList()
      .then(setItems)
      .catch(() => undefined);
  };
  useEffect(load, []);

  const rename = async (tag: TagSummary, name: string) => {
    setItems((prev) => prev.map((it) => (it.id === tag.id ? { ...it, name } : it)));
    try {
      await tagUpdate(tag.id, name.trim() || tag.name, tag.color);
    } catch {
      /* noop */
    }
  };

  const recolor = async (tag: TagSummary, color: string) => {
    setItems((prev) => prev.map((it) => (it.id === tag.id ? { ...it, color } : it)));
    try {
      await tagUpdate(tag.id, tag.name, color);
    } catch {
      /* noop */
    }
  };

  const add = async () => {
    try {
      const created = await tagCreate(t('tag.untitled'), pickTagColor(items.length));
      setItems((prev) => [...prev, created]);
    } catch {
      /* noop */
    }
  };

  const remove = async (id: number) => {
    setItems((prev) => prev.filter((it) => it.id !== id));
    try {
      await tagDelete(id);
    } catch {
      /* noop */
    }
    load();
  };

  const moveTag = async (tag: TagSummary, parent: number | null) => {
    setItems((prev) => prev.map((it) => (it.id === tag.id ? { ...it, parent_id: parent } : it)));
    try {
      await tagSetParent(tag.id, parent);
    } catch {
      /* noop */
    }
    load();
  };

  return (
    <div className="max-w-[460px] text-left">
      {items.length === 0 ? (
        <p className="text-sm text-white/60">{t('tag.empty')}</p>
      ) : (
        <ul className="space-y-2">
          {orderTree(items).map(({ tag, depth }) => (
            <li
              key={tag.id}
              className="flex items-center gap-2 rounded-md bg-white/10 px-3 py-2 text-sm"
              style={{ marginLeft: depth * 18 }}
            >
              {/* 配色スウォッチ（クリックでパレットを巡回） */}
              <button
                onClick={() => {
                  const i = TAG_PALETTE.indexOf((tag.color ?? '') as (typeof TAG_PALETTE)[number]);
                  recolor(tag, pickTagColor(i + 1));
                }}
                title={t('tag.color')}
                aria-label={t('tag.color')}
                className="h-4 w-4 shrink-0 rounded-full ring-1 ring-white/20"
                style={{ backgroundColor: tag.color ?? DEFAULT_TAG_COLOR }}
              />
              <input
                className="min-w-0 flex-1 rounded bg-transparent px-1 py-0.5 font-medium outline-none focus:bg-white/10"
                placeholder={t('tag.namePlaceholder')}
                value={tag.name}
                onChange={(e) => rename(tag, e.target.value)}
              />
              {/* 親（フォルダ）選択。自分と子孫は除外。 */}
              <select
                value={tag.parent_id ?? ''}
                onChange={(e) => moveTag(tag, e.target.value === '' ? null : Number(e.target.value))}
                title={t('tag.parent')}
                aria-label={t('tag.parent')}
                className="shrink-0 rounded bg-white/10 px-1.5 py-1 text-xs text-white/70 outline-none [color-scheme:dark]"
              >
                <option value="">{t('tag.parentNone')}</option>
                {items
                  .filter((o) => o.id !== tag.id && !isDescendant(items, o.id, tag.id))
                  .map((o) => (
                    <option key={o.id} value={o.id}>
                      {o.name}
                    </option>
                  ))}
              </select>
              <span className="shrink-0 text-xs text-white/40">
                {t('tag.count', { count: tag.count })}
              </span>
              <button
                className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/60 hover:border-red-400/60 hover:bg-red-500/30 hover:text-white"
                title={t('tag.delete')}
                aria-label={t('tag.delete')}
                onClick={() => remove(tag.id)}
              >
                <Minus size={18} />
              </button>
            </li>
          ))}
        </ul>
      )}

      <button
        onClick={add}
        title={t('tag.add')}
        aria-label={t('tag.add')}
        className="mt-3 flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
      >
        <Plus size={18} />
      </button>
    </div>
  );
}
