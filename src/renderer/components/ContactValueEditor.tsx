import { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { GripVertical, Plus, Tag, X } from 'lucide-react';
import type { CountryCode } from 'libphonenumber-js';
import type { ContactValueInput } from '@bindings/ContactValueInput';
import type { ContactAddressInput } from '@bindings/ContactAddressInput';
import { countryOptions, parseStored, toE164 } from '../utils/phone';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPostalAutoformat } from '../config/prefs';

const LABELS = ['自宅', '職場', '携帯', 'FAX', '代表'];

/** 構造化住所を 1 行の文字列へ（flat 保存・一覧用。バックエンドと同じ並び）。 */
export function addressToFlat(a: ContactAddressInput): string {
  return [a.postal, a.region, a.city, a.street, a.extended, a.country]
    .map((s) => (s ?? '').trim())
    .filter(Boolean)
    .join(' ');
}

/** 配列の from を to へ移動した新しい配列を返す。 */
function reorder<T>(list: T[], from: number, to: number): T[] {
  const next = list.slice();
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

/** ネイティブ DnD の並べ替え。ハンドルからドラッグし、行を drop 先にする。 */
function useDnd<T>(list: T[], onChange: (l: T[]) => void) {
  const [drag, setDrag] = useState<number | null>(null);
  return {
    dragging: drag,
    /** ドラッグ開始元（ハンドル）に付与。 */
    handleProps: (i: number) => ({
      draggable: true,
      onDragStart: (e: React.DragEvent) => {
        setDrag(i);
        e.dataTransfer.effectAllowed = 'move';
      },
      onDragEnd: () => setDrag(null),
    }),
    /** drop 先（各行）に付与。ホバー中の行位置へ即時に並べ替え。 */
    rowProps: (i: number) => ({
      onDragOver: (e: React.DragEvent) => {
        e.preventDefault();
        if (drag === null || drag === i) return;
        onChange(reorder(list, drag, i));
        setDrag(i);
      },
    }),
  };
}

/** ドラッグハンドル。 */
function DragHandle(props: React.HTMLAttributes<HTMLSpanElement> & { draggable?: boolean }) {
  const { t } = useTranslation();
  return (
    <span
      {...props}
      title={t('contact.reorder')}
      aria-label={t('contact.reorder')}
      className="flex h-6 w-4 shrink-0 cursor-grab items-center justify-center text-white/30 hover:text-white/70 active:cursor-grabbing"
    >
      <GripVertical size={14} />
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
  const dnd = useDnd(values, onChange);
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
          <div
            key={i}
            className={`flex items-center gap-1.5 rounded ${dnd.dragging === i ? 'opacity-50' : ''}`}
            {...dnd.rowProps(i)}
          >
            <DragHandle {...dnd.handleProps(i)} />
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

/** 電話番号の複数編集。[国]セレクト＋[国内番号]入力で、保存は E.164 正準形。 */
export function PhoneRows({
  icon,
  label,
  values,
  onChange,
}: {
  icon: React.ReactNode;
  label: string;
  values: ContactValueInput[];
  onChange: (v: ContactValueInput[]) => void;
}) {
  const { t, i18n } = useTranslation();
  const dnd = useDnd(values, onChange);
  const region = getPhoneRegion() as CountryCode;
  const countries = useMemo(() => countryOptions(i18n.language), [i18n.language]);
  const set = (i: number, patch: Partial<ContactValueInput>) =>
    onChange(values.map((v, idx) => (idx === i ? { ...v, ...patch } : v)));
  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
        {icon}
        {label}
      </span>
      <div className="space-y-1.5">
        {values.map((v, i) => {
          const parsed = parseStored(v.value, region);
          return (
            <div
              key={i}
              className={`flex items-center gap-1.5 rounded ${dnd.dragging === i ? 'opacity-50' : ''}`}
              {...dnd.rowProps(i)}
            >
              <DragHandle {...dnd.handleProps(i)} />
              <input
                className="w-16 shrink-0 rounded bg-white/10 px-2 py-1.5 text-xs outline-none focus:bg-white/15"
                placeholder={t('contact.labelPlaceholder')}
                list="contact-label-options"
                value={v.label ?? ''}
                onChange={(e) =>
                  set(i, { label: e.target.value.trim() === '' ? null : e.target.value })
                }
              />
              <select
                className="w-20 shrink-0 rounded bg-white/10 px-1 py-1.5 text-xs outline-none focus:bg-white/15"
                title={t('contact.country')}
                value={parsed.region}
                onChange={(e) =>
                  set(i, { value: toE164(parsed.national, e.target.value as CountryCode) })
                }
              >
                {countries.map((c) => (
                  <option key={c.region} value={c.region} className="text-black">
                    {c.region} +{c.calling}
                  </option>
                ))}
              </select>
              <input
                type="tel"
                className="min-w-0 flex-1 rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15"
                value={parsed.national}
                onChange={(e) => set(i, { value: e.target.value })}
                onBlur={() => set(i, { value: toE164(v.value, parsed.region) })}
              />
              <button
                onClick={() => onChange(values.filter((_, idx) => idx !== i))}
                className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                aria-label={t('contact.removeRow')}
              >
                <X size={14} />
              </button>
            </div>
          );
        })}
      </div>
      <button
        onClick={() => onChange([...values, { label: null, value: '' }])}
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
  const dnd = useDnd(addresses, onChange);
  // 郵便番号の整形基準は既定の国（自動整形オフなら素通し）。
  const postalRegion = getPostalAutoformat() ? getPhoneRegion() : '';
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
          <div
            key={i}
            className={`rounded-md border border-white/10 bg-white/5 p-2 ${
              dnd.dragging === i ? 'opacity-50' : ''
            }`}
            {...dnd.rowProps(i)}
          >
            <div className="mb-1.5 flex items-center gap-2">
              <DragHandle {...dnd.handleProps(i)} />
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
              <button
                onClick={() => onChange(addresses.filter((_, idx) => idx !== i))}
                className="flex h-6 w-6 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                aria-label={t('contact.removeRow')}
              >
                <X size={13} />
              </button>
            </div>
            <div className="grid grid-cols-2 gap-1.5">
              <input
                className="rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
                placeholder={t('contact.postal')}
                value={a.postal ?? ''}
                onChange={(e) => set(i, { postal: e.target.value.trim() === '' ? null : e.target.value })}
                onBlur={(e) => {
                  const f = formatPostal(e.target.value, postalRegion);
                  set(i, { postal: f === '' ? null : f });
                }}
              />
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
