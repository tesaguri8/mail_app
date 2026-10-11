import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, ChevronDown, Plus } from 'lucide-react';
import type { CountryCode } from 'libphonenumber-js';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrgMatch } from '@bindings/OrgMatch';
import {
  organizationCreateFromName,
  organizationCreateFromNameImpact,
  organizationMatches,
} from '../services/organizations';
import { getPhoneRegion, getPhoneStyle } from '../config/prefs';
import { displayPhone } from '../utils/phone';
import { orgMatchKey, orgMatchReasonLabel, othersLinked } from '../utils/orgMatch';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 似ている組織を探す手掛かり（この連絡先の id・メール・電話）。 */
export interface OrgRegisterContext {
  contactId: number | null;
  emails: readonly string[];
  phones: readonly string[];
}

/**
 * 組織カードにつながっていない会社名の下に出す「組織に登録」と、似ている既存の組織の候補。
 *
 * - 「組織に登録」: その場でその名前の組織カードを作ってつなぐ（組織タブの「整理」と同じ動き＝
 *   同じ会社名でカードにつながっていない人も一緒につなぐ。同じ名前のカードが既にあればそこへ）。
 *   押した時点で DB に書く（`onLinked` で親の下書きにも反映する）
 * - 候補: 名前・メールのドメイン・電話が似ている既存の組織。選ぶと下書きをその組織にする
 *   （保存でつながる。ふつうの候補選びと同じ）
 */
export function OrgRegister({
  name,
  context,
  onLinked,
  onPick,
  onMatchCount,
}: {
  name: string;
  context: OrgRegisterContext;
  /** 「組織に登録」でつないだあと（DB に書き込み済み）。 */
  onLinked: (org: OrganizationSummary) => void;
  /** 候補を選んだとき（下書きだけを変える）。 */
  onPick: (org: OrganizationSummary) => void;
  /** 似た組織の件数が届いたとき（`forKey` はどの会社名・どの人の結果か＝`orgMatchKey`）。欄の色に使う。 */
  onMatchCount?: (forKey: string, count: number) => void;
}) {
  const { t } = useTranslation();
  const [matches, setMatches] = useState<OrgMatch[]>([]);
  const [others, setOthers] = useState(0);
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);
  const region = getPhoneRegion() as CountryCode;
  const style = getPhoneStyle();

  // 候補と「ほか N 人」を取る（会社名・メール・電話が変わったら取り直す）。
  const key = `${name}|${context.contactId ?? ''}|${context.emails.join(',')}|${context.phones.join(',')}`;
  useEffect(() => {
    if (!isTauri) return;
    let alive = true;
    const h = setTimeout(() => {
      organizationMatches(name, [...context.emails], [...context.phones], context.contactId)
        .then((m) => {
          if (!alive) return;
          setMatches(m);
          onMatchCount?.(orgMatchKey(name, context.contactId), m.length);
        })
        .catch(() => {
          if (!alive) return;
          setMatches([]);
          onMatchCount?.(orgMatchKey(name, context.contactId), 0);
        });
      organizationCreateFromNameImpact(name)
        .then((impact) => alive && setOthers(othersLinked(impact.linked)))
        .catch(() => alive && setOthers(0));
    }, 200);
    return () => {
      alive = false;
      clearTimeout(h);
    };
    // key に必要な値をまとめている。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // 外側を押すと候補を閉じる。
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, [open]);

  const register = async () => {
    setBusy(true);
    setFailed(false);
    try {
      onLinked(await organizationCreateFromName(name));
    } catch {
      setFailed(true);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div ref={boxRef} className="relative mt-1.5">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
        <button
          type="button"
          onClick={register}
          disabled={busy}
          className="flex items-center gap-1 rounded bg-sky-500/20 px-2 py-1 text-[11px] text-sky-100 hover:bg-sky-500/30 disabled:opacity-50"
        >
          <Plus size={12} />
          {t('contact.orgRegister')}
        </button>
        {matches.length > 0 && (
          <button
            type="button"
            onClick={() => setOpen((v) => !v)}
            aria-expanded={open}
            className="flex items-center gap-1 rounded px-2 py-1 text-[11px] text-white/70 hover:bg-white/10"
          >
            <Building2 size={12} />
            {t('contact.orgMatches', { count: matches.length })}
            <ChevronDown size={12} />
          </button>
        )}
        {others > 0 && (
          <span className="text-[11px] text-white/45">
            {t('contact.orgRegisterOthers', { count: others })}
          </span>
        )}
      </div>
      {failed && <p className="mt-1 text-[11px] text-red-300">{t('contact.orgRegisterFailed')}</p>}
      {open && (
        <ul className="absolute left-0 right-0 top-full z-30 mt-1 max-h-72 overflow-y-auto rounded-md border border-white/15 bg-neutral-900/95 py-1 text-xs shadow-lg backdrop-blur">
          {matches.map((m) => (
            <li key={m.org.id}>
              <button
                type="button"
                onClick={() => {
                  setOpen(false);
                  onPick(m.org);
                }}
                className="w-full px-3 py-1.5 text-left hover:bg-white/10"
              >
                <span className="flex items-baseline gap-2">
                  <span className="min-w-0 truncate font-medium text-white/90">{m.org.name}</span>
                  <span className="shrink-0 text-[10px] text-white/40">
                    {t('contact.orgMembers', { count: m.org.member_count })}
                  </span>
                </span>
                <span className="mt-0.5 block text-[10px] text-amber-200/80">
                  {m.reasons
                    .map((r) => {
                      const { key: k, values } = orgMatchReasonLabel(r, (p) =>
                        displayPhone(p, style, region)
                      );
                      return t(k, values);
                    })
                    .join('・')}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
