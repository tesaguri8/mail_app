import { useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { X } from 'lucide-react';
import type { PostalAddress } from '@bindings/PostalAddress';
import { formatPostal } from '../utils/postal';
import { getPhoneRegion, getPostalAutoformat } from '../config/prefs';
import { postalLookupByAddress, postalLookupByCode } from '../services/postal';
import {
  patchFromAddress,
  patchFromCode,
  wantsAddressFromCode,
  wantsCodeFromAddress,
  type PostalFields,
} from '../utils/postalAutofill';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 候補から選ばせる中身（郵便番号から住所を引いたか、住所から郵便番号を引いたか）。 */
type Candidates = { kind: 'address' | 'code'; list: PostalAddress[] };

const nullify = (v: string) => (v.trim() === '' ? null : v);

/**
 * 住所の入力欄（郵便番号・都道府県・市区町村・町域・建物・国）。連絡先の住所と組織の所在地で共通。
 *
 * 日本の住所なら郵便番号を自動で入れる（同梱の郵便番号表で引き、住所は外部へ送らない）:
 * - 郵便番号を 7 桁入れて、都道府県・市区町村・町域が空 → 住所を入れる
 * - 郵便番号が空で、都道府県・市区町村・町域を入れ終えた（欄を離れた）→ 郵便番号を入れる
 * 入っている値は上書きしない。候補が複数なら勝手に選ばず、下に並べて選ばせる。
 */
export function AddressFields({
  value,
  onPatch,
  countryCode,
}: {
  value: PostalFields;
  onPatch: (patch: Partial<PostalFields>) => void;
  /** 国コード（連絡先の住所のみ。'JP' 以外なら自動入力しない）。 */
  countryCode?: string | null;
}) {
  const { t } = useTranslation();
  const [candidates, setCandidates] = useState<Candidates | null>(null);
  // 検索は非同期なので、結果を入れるときは最新の値・最新の onPatch で判断する
  // （待つ間に打った文字を、古い値で上書きしないため）。
  const latest = useRef({ value, onPatch });
  latest.current = { value, onPatch };
  // 郵便番号の整形基準は既定の国（自動整形オフなら素通し）。
  const postalRegion = getPostalAutoformat() ? getPhoneRegion() : '';

  const apply = (kind: Candidates['kind'], found: PostalAddress) => {
    const { value: now, onPatch: patch } = latest.current;
    const diff = kind === 'address' ? patchFromAddress(now, found) : patchFromCode(now, found);
    if (Object.keys(diff).length > 0) patch(diff);
  };

  const offer = (kind: Candidates['kind'], list: PostalAddress[]) => {
    if (list.length === 1) {
      apply(kind, list[0]);
      setCandidates(null);
    } else {
      setCandidates(list.length > 1 ? { kind, list } : null);
    }
  };

  const changePostal = (raw: string) => {
    const next = { ...value, postal: nullify(raw) };
    onPatch({ postal: next.postal });
    if (!isTauri || !wantsAddressFromCode(next, countryCode)) return;
    postalLookupByCode(raw)
      .then((list) => {
        // 待つ間に住所が入っていたら何もしない。
        if (wantsAddressFromCode(latest.current.value, countryCode)) offer('address', list);
      })
      .catch(() => undefined);
  };

  // 都道府県・市区町村・町域の欄を離れたとき、そろっていれば郵便番号を引く。
  const lookupCode = () => {
    const now = latest.current.value;
    if (!isTauri || !wantsCodeFromAddress(now, countryCode)) return;
    postalLookupByAddress(now.region ?? '', now.city ?? '', now.street ?? '')
      .then((list) => {
        if (wantsCodeFromAddress(latest.current.value, countryCode)) offer('code', list);
      })
      .catch(() => undefined);
  };

  const field = (key: keyof PostalFields, ph: string, w = '', onBlur?: () => void) => (
    <input
      className={`rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15 ${w}`}
      placeholder={ph}
      value={value[key] ?? ''}
      onChange={(e) => onPatch({ [key]: nullify(e.target.value) })}
      onBlur={onBlur}
    />
  );

  return (
    <>
      <div className="grid grid-cols-2 gap-1.5">
        <input
          className="rounded bg-white/10 px-2 py-1.5 text-sm outline-none focus:bg-white/15"
          placeholder={t('contact.postal')}
          value={formatPostal(value.postal ?? '', postalRegion)}
          onChange={(e) => changePostal(e.target.value)}
        />
        {field('region', t('contact.region'), '', lookupCode)}
        {field('city', t('contact.city'), '', lookupCode)}
        {field('street', t('contact.street'), '', lookupCode)}
        {field('extended', t('contact.extended'), 'col-span-2')}
        {field('country', t('contact.country'), 'col-span-2')}
      </div>
      {candidates && (
        <div className="mt-1.5 rounded-md border border-sky-300/20 bg-sky-400/10 p-1.5">
          <div className="mb-1 flex items-center justify-between gap-2 px-1 text-[11px] text-sky-100/80">
            {t(candidates.kind === 'address' ? 'contact.postalPickAddress' : 'contact.postalPickCode')}
            <button
              onClick={() => setCandidates(null)}
              title={t('contact.postalDismiss')}
              aria-label={t('contact.postalDismiss')}
              className="flex h-5 w-5 items-center justify-center rounded-full text-white/50 hover:bg-white/10 hover:text-white"
            >
              <X size={12} />
            </button>
          </div>
          <ul className="max-h-40 overflow-y-auto">
            {candidates.list.map((c) => (
              <li key={`${c.postal}|${c.town}`}>
                <button
                  onClick={() => {
                    apply(candidates.kind, c);
                    setCandidates(null);
                  }}
                  className="flex w-full items-baseline gap-2 rounded px-1.5 py-1 text-left text-xs hover:bg-white/10"
                >
                  <span className="shrink-0 tabular-nums text-white/70">〒{c.postal}</span>
                  <span className="truncate">
                    {c.region}
                    {c.city}
                    {c.town || t('contact.postalTownOther')}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </>
  );
}
