import { useTranslation } from 'react-i18next';
import { Plus, X } from 'lucide-react';
import type { ContactHandle } from '@bindings/ContactHandle';
import type { HandleKind } from '@bindings/HandleKind';
import { DragHandle, useDnd } from './ContactValueEditor';
import { LABEL_LIST_IDS } from '../utils/contactLabels';

const INPUT = 'rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15';
const INPUT_INVALID =
  'rounded bg-red-500/15 px-2.5 py-1.5 text-sm text-red-100 outline-none ring-1 ring-red-400/70 focus:bg-red-500/20';

/** 1 行の入力欄の定義（どのキーを、どんな見た目で編集するか）。 */
type Column<T> = {
  key: keyof T & string;
  placeholder: string;
  /** 候補の datalist の id。 */
  list?: string;
  /** 幅のクラス（既定は残りを埋める）。 */
  width?: string;
  /** 値が不正なら true（赤枠で知らせる。保存は止めない）。 */
  invalid?: (v: string) => boolean;
};

/** 見出しの行（アイコン＋名前）。他の行エディタと同じ見た目。 */
function RowsHeading({ icon, label }: { icon: React.ReactNode; label: string }) {
  return (
    <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
      {icon}
      {label}
    </span>
  );
}

function RemoveButton({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  return (
    <button
      onClick={onClick}
      className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
      aria-label={t('contact.removeRow')}
    >
      <X size={14} />
    </button>
  );
}

function AddButton({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <button
      onClick={onClick}
      className="mt-1.5 flex items-center gap-1 text-xs text-sky-300 hover:text-sky-200"
    >
      <Plus size={13} />
      {label}
    </button>
  );
}

/**
 * 文字列 2 つ（ラベル＋値など）の行の複数編集。URL・日付・関係・カスタム項目で共有する。
 * 空のキーは null（任意項目）か ''（必須項目）に寄せて保存する — どちらかは `nullable` で決める。
 */
export function PairRows<T extends object>({
  icon,
  label,
  items,
  onChange,
  empty,
  columns,
  nullable,
}: {
  icon: React.ReactNode;
  label: string;
  items: T[];
  onChange: (items: T[]) => void;
  empty: () => T;
  columns: [Column<T>, Column<T>];
  /** 空にしたとき null にするキー（それ以外は '' のまま）。 */
  nullable: ReadonlyArray<keyof T & string>;
}) {
  const { t } = useTranslation();
  const dnd = useDnd(items, onChange);
  const set = (i: number, key: keyof T & string, raw: string) => {
    const v = raw.trim() === '' && nullable.includes(key) ? null : raw;
    onChange(items.map((it, idx) => (idx === i ? { ...it, [key]: v } : it)));
  };
  const text = (it: T, key: keyof T & string): string => {
    const v = it[key];
    return typeof v === 'string' ? v : '';
  };
  return (
    <div>
      <RowsHeading icon={icon} label={label} />
      <div className="space-y-1.5">
        {items.map((it, i) => (
          <div
            key={i}
            className={`flex items-center gap-1.5 rounded ${dnd.dragging === i ? 'opacity-50' : ''}`}
            {...dnd.rowProps(i)}
          >
            <DragHandle {...dnd.handleProps(i)} />
            {columns.map((c) => {
              const v = text(it, c.key);
              const bad = v !== '' && (c.invalid?.(v) ?? false);
              return (
                <input
                  key={c.key}
                  className={`${bad ? INPUT_INVALID : INPUT} ${c.width ?? 'min-w-0 flex-1'}`}
                  placeholder={c.placeholder}
                  list={c.list}
                  value={v}
                  onChange={(e) => set(i, c.key, e.target.value)}
                />
              );
            })}
            <RemoveButton onClick={() => onChange(items.filter((_, idx) => idx !== i))} />
          </div>
        ))}
      </div>
      <AddButton onClick={() => onChange([...items, empty()])} label={t('contact.addRow')} />
    </div>
  );
}

const emptyHandle = (): ContactHandle => ({ kind: 'im', service: null, value: '', label: null });

/**
 * チャット・SNS のハンドルの複数編集。種類（チャット／SNS）・サービス名・ユーザー名。
 * Google へ送られるのはチャットだけ（SNS は iCloud の項目）なので、種類の選択肢に添えて示す。
 */
export function HandleRows({
  icon,
  label,
  handles,
  onChange,
}: {
  icon: React.ReactNode;
  label: string;
  handles: ContactHandle[];
  onChange: (h: ContactHandle[]) => void;
}) {
  const { t } = useTranslation();
  const dnd = useDnd(handles, onChange);
  const set = (i: number, patch: Partial<ContactHandle>) =>
    onChange(handles.map((h, idx) => (idx === i ? { ...h, ...patch } : h)));
  return (
    <div>
      <RowsHeading icon={icon} label={label} />
      <div className="space-y-1.5">
        {handles.map((h, i) => (
          <div
            key={i}
            className={`flex items-center gap-1.5 rounded ${dnd.dragging === i ? 'opacity-50' : ''}`}
            {...dnd.rowProps(i)}
          >
            <DragHandle {...dnd.handleProps(i)} />
            <select
              className="w-20 shrink-0 rounded bg-white/10 px-1 py-1.5 text-xs text-white outline-none focus:bg-white/15"
              value={h.kind}
              title={t('contact.handleKindHint')}
              onChange={(e) => set(i, { kind: e.target.value as HandleKind })}
            >
              <option value="im" className="bg-neutral-800">
                {t('contact.handleIm')}
              </option>
              <option value="social" className="bg-neutral-800">
                {t('contact.handleSocial')}
              </option>
            </select>
            <input
              className={`${INPUT} w-24 shrink-0 text-xs`}
              placeholder={t('contact.handleService')}
              list={h.kind === 'im' ? LABEL_LIST_IDS.im : LABEL_LIST_IDS.social}
              value={h.service ?? ''}
              onChange={(e) =>
                set(i, { service: e.target.value.trim() === '' ? null : e.target.value })
              }
            />
            <input
              className={`${INPUT} min-w-0 flex-1`}
              placeholder={t('contact.handleValue')}
              value={h.value}
              onChange={(e) => set(i, { value: e.target.value })}
            />
            <RemoveButton onClick={() => onChange(handles.filter((_, idx) => idx !== i))} />
          </div>
        ))}
      </div>
      <AddButton
        onClick={() => onChange([...handles, emptyHandle()])}
        label={t('contact.addRow')}
      />
    </div>
  );
}
