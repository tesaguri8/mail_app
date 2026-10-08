import { useTranslation } from 'react-i18next';
import { Bird, Cloud } from 'lucide-react';
import type { ContactLink } from '@bindings/ContactLink';
import type { ContactProvider } from '@bindings/ContactProvider';

/**
 * 連絡先がどのサービスと同期しているか（docs/CONTACT_MODEL.md §2）。
 * 商標のロゴは使わず、汎用の印＋文字で区別する（Google＝「G」、iCloud＝雲、どこにも
 * つながっていなければ Rondine＝燕）。
 */

/** 一覧の絞り込み（すべて / サービスごと / どこにもつながっていない）。 */
export type ContactSourceFilter = 'all' | ContactProvider | 'rondine';

export const CONTACT_SOURCE_FILTERS: ContactSourceFilter[] = ['all', 'google', 'icloud', 'rondine'];

/** 絞り込みに合うか。 */
export const matchesSource = (links: ContactLink[], f: ContactSourceFilter): boolean => {
  if (f === 'all') return true;
  if (f === 'rondine') return links.length === 0;
  return links.some((l) => l.provider === f);
};

function ProviderMark({ provider, size }: { provider: ContactProvider | 'rondine'; size: number }) {
  switch (provider) {
    case 'google':
      return (
        <span
          className="flex shrink-0 items-center justify-center rounded-full bg-sky-400/25 font-bold leading-none text-sky-100"
          style={{ width: size, height: size, fontSize: Math.round(size * 0.62) }}
        >
          G
        </span>
      );
    case 'icloud':
      return <Cloud size={size} className="shrink-0 text-sky-200" />;
    case 'rondine':
      return <Bird size={size} className="shrink-0 text-white/40" />;
  }
}

/** 一覧の行に添える小さな印（同じサービスは 1 つにまとめる）。 */
export function ContactLinkMarks({ links }: { links: ContactLink[] }) {
  const { t } = useTranslation();
  const providers = [...new Set(links.map((l) => l.provider))];
  if (providers.length === 0) {
    return (
      <span title={t('contact.link.rondineOnly')} aria-label={t('contact.link.rondineOnly')}>
        <ProviderMark provider="rondine" size={13} />
      </span>
    );
  }
  return (
    <span className="flex shrink-0 items-center gap-1">
      {providers.map((p) => {
        const mine = links.filter((l) => l.provider === p);
        const title = mine
          .map(
            (l) =>
              `${t(`contact.link.${p}`)}${l.account_email ? ` · ${l.account_email}` : ''}` +
              (l.disconnected ? ` (${t('contact.link.disconnected')})` : ''),
          )
          .join('\n');
        // そのサービスのつながりがすべて解除中なら薄く出す（記録は残っているが同期しない）。
        const dim = mine.every((l) => l.disconnected);
        return (
          <span key={p} title={title} aria-label={title} className={dim ? 'opacity-35' : undefined}>
            <ProviderMark provider={p} size={13} />
          </span>
        );
      })}
    </span>
  );
}

/** 詳細（編集画面）の見出しに並べる、アカウント名つきの印。 */
export function ContactLinkChips({ links }: { links: ContactLink[] }) {
  const { t } = useTranslation();
  const chip =
    'flex items-center gap-1.5 rounded-full bg-white/10 px-2.5 py-0.5 text-[11px] text-white/70';
  if (links.length === 0) {
    return (
      <div className="flex flex-wrap gap-1.5">
        <span className={chip}>
          <ProviderMark provider="rondine" size={12} />
          {t('contact.link.rondineOnly')}
        </span>
      </div>
    );
  }
  return (
    <div className="flex flex-wrap gap-1.5">
      {links.map((l) => (
        <span
          key={`${l.provider}-${l.account_id}`}
          className={`${chip} ${l.disconnected ? 'opacity-60' : ''}`}
          title={l.disconnected ? t('contact.link.disconnectedHint') : undefined}
        >
          <ProviderMark provider={l.provider} size={12} />
          {t(`contact.link.${l.provider}`)}
          {l.account_email && <span className="text-white/45">{l.account_email}</span>}
          {l.disconnected && (
            <span className="rounded bg-amber-400/20 px-1 text-[10px] text-amber-200">
              {t('contact.link.disconnected')}
            </span>
          )}
        </span>
      ))}
    </div>
  );
}
