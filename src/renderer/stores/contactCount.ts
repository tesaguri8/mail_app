import { create } from 'zustand';

/** 連絡先の一覧に出している件数（フッターの左に出す）。 */
export interface ContactCount {
  /** いま一覧に出している人数。 */
  shown: number;
  /** 生きている人の総数（ゴミ箱を見ているときは使わない）。 */
  total: number;
  /** ゴミ箱を見ているか。 */
  trash: boolean;
}

interface ContactCountState {
  /** null は連絡先の一覧を出していない（またはまだ読んでいない）。 */
  count: ContactCount | null;
  set: (count: ContactCount | null) => void;
}

/**
 * 連絡先の一覧が出している件数。一覧（ContactsView）が書き、フッター（BottomBar）が読む。
 * 数え直しはせず、一覧に出している分から出す。
 */
export const useContactCountStore = create<ContactCountState>((set) => ({
  count: null,
  set: (count) => set({ count }),
}));

/** フッターに出す言い回し（翻訳キーと差し込む値）。数は 3 桁区切りにする。 */
export function contactCountLabel(c: ContactCount): {
  key: 'contact.trashTotal' | 'contact.listTotal' | 'contact.listTotalOf';
  values: Record<string, string>;
} {
  if (c.trash) return { key: 'contact.trashTotal', values: { total: c.shown.toLocaleString() } };
  if (c.shown !== c.total) {
    return {
      key: 'contact.listTotalOf',
      values: { shown: c.shown.toLocaleString(), total: c.total.toLocaleString() },
    };
  }
  return { key: 'contact.listTotal', values: { total: c.total.toLocaleString() } };
}
