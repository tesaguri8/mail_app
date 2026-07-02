import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, ChevronUp, Plus, Tag, X } from 'lucide-react';
import type { ContactValueInput } from '@bindings/ContactValueInput';
import type { ContactAddressInput } from '@bindings/ContactAddressInput';

const LABELS = ['自宅', '職場', '携帯', 'FAX', '代表'];

/** 構造化住所を 1 行の文字列へ（flat 保存・一覧用。バックエンドと同じ並び）。 */
export function addressToFlat(a: ContactAddressInput): string {
  return [a.postal, a.region, a.city, a.street, a.extended, a.country]
    .map((s) => (s ?? '').trim())
    .filter(Boolean)
    .join(' ');
}

/** 配列 i 番目を dir(-1/+1) 方向へ入れ替えた新しい配列を返す（範囲外はそのまま）。 */
function moved<T>(list: T[], i: number, dir: number): T[] {
  const j = i + dir;
  if (j < 0 || j >= list.length) return list;
  const next = list.slice();
  [next[i], next[j]] = [next[j], next[i]];
  return next;
}

/** 行の上下移動ボタン（並べ替え。先頭が主値になる）。 */
function MoveButtons({
  onUp,
  onDown,
  upDisabled,
  downDisabled,
}: {
  onUp: () => void;
  onDown: () => void;
  upDisabled: boolean;
  downDisabled: boolean;
}) {
  const { t } = useTranslation();
  const cls =
    'flex h-5 w-5 items-center justify-center rounded text-white/40 hover:bg-white/10 hover:text-white disabled:opacity-25 disabled:hover:bg-transparent';
  return (
    <span className="flex shrink-0 flex-col">
      <button onClick={onUp} disabled={upDisabled} className={cls} aria-label={t('contact.moveUp')}>
        <ChevronUp size={13} />
      </button>
      <button
        onClick={onDown}
        disabled={downDisabled}
        className={cls}
        aria-label={t('contact.moveDown')}
      >
        <ChevronDown size={13} />
      </button>
    </span>
  );
}

const emptyValue = (): ContactValueInput => ({ label: null, value: '' });
const emptyAddress = (): ContactAddressInput => ({
  label: null,
  postal: null,
  region: null,
  city: null,
  street: null,
  extended: null,
  country: null,
});

/** メール/電話などラベル付き複数値の編集（＋追加・−削除・ラベル候補）。 */
export function ValueRows({
  icon,
  label,
  values,
  onChange,
  inputType = 'text',
}: {
  icon: React.ReactNode;
  label: string;
  values: ContactValueInput[];
  onChange: (v: ContactValueInput[]) => void;
  inputType?: string;
}) {
  const { t } = useTranslation();
  const set = (i: number, patch: Partial<ContactValueInput>) =>
    onChange(values.map((v, idx) => (idx === i ? { ...v, ...patch } : v)));
  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
        {icon}
        {label}
      </span>
      <div className="space-y-1.5">
        {values.map((v, i) => (
          <div key={i} className="flex items-center gap-2">
            <input
              className="w-16 shrink-0 rounded bg-white/10 px-2 py-1.5 text-xs outline-none focus:bg-white/15"
              placeholder={t('contact.labelPlaceholder')}
              list="contact-label-options"
              value={v.label ?? ''}
              onChange={(e) => set(i, { label: e.target.value.trim() === '' ? null : e.target.value })}
            />
            <input
              type={inputType}
              className="min-w-0 flex-1 rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
              value={v.value}
              onChange={(e) => set(i, { value: e.target.value })}
            />
            <MoveButtons
              onUp={() => onChange(moved(values, i, -1))}
              onDown={() => onChange(moved(values, i, 1))}
              upDisabled={i === 0}
              downDisabled={i === values.length - 1}
            />
            <button
              onClick={() => onChange(values.filter((_, idx) => idx !== i))}
              className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
              aria-label={t('contact.removeRow')}
            >
              <X size={14} />
            </button>
          </div>
        ))}
      </div>
      <button
        onClick={() => onChange([...values, emptyValue()])}
        className="mt-1.5 flex items-center gap-1 text-xs text-sky-300 hover:text-sky-200"
      >
        <Plus size={13} />
        {t('contact.addRow')}
      </button>
      <datalist id="contact-label-options">
        {LABELS.map((l) => (
          <option key={l} value={l} />
        ))}
      </datalist>
    </div>
  );
}

/** タグ編集（チップ＋オートコンプリート）。tags は名前の配列。 */
export function TagInput({
  tags,
  onChange,
  suggestions,
}: {
  tags: string[];
  onChange: (t: string[]) => void;
  suggestions: string[];
}) {
  const { t } = useTranslation();
  const [text, setText] = useState('');
  const add = (name: string) => {
    const n = name.trim();
    if (n && !tags.includes(n)) onChange([...tags, n]);
    setText('');
  };
  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
        <Tag size={13} />
        {t('contact.tags')}
      </span>
      <div className="flex flex-wrap items-center gap-1.5">
        {tags.map((tag) => (
          <span
            key={tag}
            className="flex items-center gap-1 rounded-full bg-sky-500/25 px-2 py-0.5 text-xs text-sky-100"
          >
            {tag}
            <button
              onClick={() => onChange(tags.filter((x) => x !== tag))}
              className="text-sky-200/70 hover:text-white"
              aria-label={t('contact.removeRow')}
            >
              <X size={11} />
            </button>
          </span>
        ))}
        <input
          className="min-w-[8rem] flex-1 rounded bg-white/10 px-2 py-1 text-sm outline-none focus:bg-white/15"
          placeholder={t('contact.addTag')}
          list="contact-tag-suggestions"
          value={text}
          onChange={(e) => {
            // datalist から確定選択されたら即追加。
            const v = e.target.value;
            if (suggestions.includes(v)) add(v);
            else setText(v);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault();
              add(text);
            }
          }}
          onBlur={() => add(text)}
        />
      </div>
      <datalist id="contact-tag-suggestions">
        {suggestions.map((s) => (
          <option key={s} value={s} />
        ))}
      </datalist>
    </div>
  );
}

/** 構造化住所の複数編集。 */
export function AddressRows({
  icon,
  label,
  addresses,
  onChange,
}: {
  icon: React.ReactNode;
  label: string;
  addresses: ContactAddressInput[];
  onChange: (a: ContactAddressInput[]) => void;
}) {
  const { t } = useTranslation();
  const set = (i: number, patch: Partial<ContactAddressInput>) =>
    onChange(addresses.map((a, idx) => (idx === i ? { ...a, ...patch } : a)));
  const field = (i: number, key: keyof ContactAddressInput, ph: string, w = '') => (
    <input
      className={`rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15 ${w}`}
      placeholder={ph}
      value={(addresses[i][key] as string | null) ?? ''}
      onChange={(e) => set(i, { [key]: e.target.value.trim() === '' ? null : e.target.value })}
    />
  );
  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
        {icon}
        {label}
      </span>
      <div className="space-y-2">
        {addresses.map((a, i) => (
          <div key={i} className="rounded-md border border-white/10 bg-white/5 p-2">
            <div className="mb-1.5 flex items-center gap-2">
              <input
                className="w-20 rounded bg-white/10 px-2 py-1 text-xs outline-none focus:bg-white/15"
                placeholder={t('contact.labelPlaceholder')}
                list="contact-label-options"
                value={a.label ?? ''}
                onChange={(e) =>
                  set(i, { label: e.target.value.trim() === '' ? null : e.target.value })
                }
              />
              <span className="flex-1" />
              <MoveButtons
                onUp={() => onChange(moved(addresses, i, -1))}
                onDown={() => onChange(moved(addresses, i, 1))}
                upDisabled={i === 0}
                downDisabled={i === addresses.length - 1}
              />
              <button
                onClick={() => onChange(addresses.filter((_, idx) => idx !== i))}
                className="flex h-6 w-6 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                aria-label={t('contact.removeRow')}
              >
                <X size={13} />
              </button>
            </div>
            <div className="grid grid-cols-2 gap-1.5">
              {field(i, 'postal', t('contact.postal'))}
              {field(i, 'region', t('contact.region'))}
              {field(i, 'city', t('contact.city'))}
              {field(i, 'street', t('contact.street'))}
              {field(i, 'extended', t('contact.extended'), 'col-span-2')}
              {field(i, 'country', t('contact.country'), 'col-span-2')}
            </div>
          </div>
        ))}
      </div>
      <button
        onClick={() => onChange([...addresses, emptyAddress()])}
        className="mt-1.5 flex items-center gap-1 text-xs text-sky-300 hover:text-sky-200"
      >
        <Plus size={13} />
        {t('contact.addAddress')}
      </button>
    </div>
  );
}
