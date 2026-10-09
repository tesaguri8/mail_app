import { create } from 'zustand';
import type { ContactSummary } from '@bindings/ContactSummary';
import { contactList } from '../services/contacts';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 連絡先一覧の手元の写し（検索語・タグ絞り込み・ゴミ箱表示なしの既定の一覧）。
 *
 * 連絡先タブは入るたびに作り直されるので、毎回空から取りに行くと待たされる。ここに置いた分を
 * すぐ出し、裏で取り直して差し替える（stale-while-revalidate）。取り直しの契機は、タブに
 * 入ったとき・自動同期・保存・削除・復元・取り込み・統合と、アプリ起動後の先読み。
 */
interface ContactsState {
  /** 既定の一覧。null はまだ一度も読んでいない。 */
  items: ContactSummary[] | null;
  /** 一覧を取り直して差し替える。重なったときは最後に頼んだ分だけを反映する。 */
  refresh: () => Promise<void>;
  /** 取り直しを待たずに、手元の一覧から外す（削除の直後に消えて見えるように）。 */
  remove: (id: number) => void;
}

/** 取り直しの通し番号。保存直後の取り直しを、それより前に出した遅い応答で上書きしないため。 */
let seq = 0;

export const useContactsStore = create<ContactsState>((set) => ({
  items: null,
  refresh: () => {
    if (!isTauri) return Promise.resolve();
    const mine = ++seq;
    return contactList('', [], false)
      .then((items) => {
        if (mine === seq) set({ items });
      })
      .catch(() => undefined);
  },
  remove: (id) => set((s) => ({ items: s.items?.filter((c) => c.id !== id) ?? null })),
}));
