import { useCallback, useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Building2, Gem, Mail, Pencil, Phone, Search, User, Users, Wand2, X } from 'lucide-react';
import type { OrganizationSummary } from '@bindings/OrganizationSummary';
import type { OrganizationDetail } from '@bindings/OrganizationDetail';
import { organizationDetail, organizationList } from '../services/organizations';
import { OrgCardInfo } from './OrgCard';
import { OrgTidy } from './OrgTidy';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 住所録の「組織」タブ（見る・つなぐ場所。docs/ORG_CONTACTS.md §4-3）。左＝組織一覧（検索）、
 * 右＝代表の情報・共有アドレス（共有件数つき）・メンバー。組織も連絡先の 1 件なので、
 * 名前や代表の情報を直すのは［連絡先で編集］から連絡先の画面で行う（ここに編集の画面は持たない）。
 * つなぐ・まとめるは「整理」で行う。
 */
export function OrganizationsView({
  focusId,
  onOpenContact,
}: {
  /** 起動時に開く組織（重複バナー等からの遷移用）。 */
  focusId?: number | null;
  /** メンバー・組織自身を連絡先タブで開く。 */
  onOpenContact?: (id: number) => void;
}) {
  const { t } = useTranslation();
  const [items, setItems] = useState<OrganizationSummary[]>([]);
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [detail, setDetail] = useState<OrganizationDetail | null>(null);
  // 「整理」（組織になっていない会社名・つながっていない人・重複・会社名だけの連絡先）の画面。
  const [tidy, setTidy] = useState(false);
  // 右ペインのスクロール枠。別の組織に切り替えたときだけ先頭へ戻す。
  const paneRef = useRef<HTMLElement>(null);
  const openedIdRef = useRef<number | null>(null);

  const load = useCallback((q: string) => {
    if (!isTauri) return;
    organizationList(q, false)
      .then(setItems)
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const h = setTimeout(() => load(query), 150);
    return () => clearTimeout(h);
  }, [query, load]);

  const open = useCallback((id: number) => {
    if (!isTauri) return;
    if (openedIdRef.current !== id) paneRef.current?.scrollTo({ top: 0 });
    openedIdRef.current = id;
    setSelectedId(id);
    organizationDetail(id)
      .then(setDetail)
      .catch(() => setDetail(null));
  }, []);

  // 起動時フォーカス（重複バナーからの遷移）。
  useEffect(() => {
    if (focusId != null) open(focusId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focusId]);

  // 整理は専用の 2 ペイン画面を全幅で出す（住所録の重複整理と同じ）。
  if (tidy) {
    return (
      <OrgTidy
        onChanged={() => load(query)}
        onExit={() => setTidy(false)}
        onOpenOrg={(id) => {
          setTidy(false);
          open(id);
        }}
      />
    );
  }

  return (
    <div className="flex h-full min-h-0">
      {/* 左：検索 + 組織一覧 */}
      <aside className="flex w-72 shrink-0 flex-col border-r border-white/10">
        <div className="flex items-center gap-2 p-3">
          <div className="flex min-w-0 flex-1 items-center gap-2 rounded-md bg-white/10 px-2.5 py-1.5">
            <Search size={15} className="shrink-0 text-white/50" />
            <input
              className="min-w-0 flex-1 bg-transparent text-sm outline-none placeholder:text-white/40"
              placeholder={t('org.search')}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
            />
            {query && (
              <button
                onClick={() => setQuery('')}
                title={t('contact.clearSearch')}
                aria-label={t('contact.clearSearch')}
                className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-white/40 hover:bg-white/20 hover:text-white"
              >
                <X size={12} />
              </button>
            )}
          </div>
          <button
            onClick={() => setTidy(true)}
            title={t('orgTidy.title')}
            aria-label={t('orgTidy.title')}
            className="flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
          >
            <Wand2 size={16} />
          </button>
        </div>

        <ul className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
          {items.length === 0 ? (
            <li className="px-2 py-6 text-center text-sm text-white/45">{t('org.empty')}</li>
          ) : (
            items.map((o) => (
              <li key={o.id}>
                <button
                  onClick={() => open(o.id)}
                  className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-2 text-left ${
                    selectedId === o.id ? 'bg-white/20' : 'hover:bg-white/10'
                  }`}
                >
                  <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-md bg-white/15">
                    <Building2 size={15} />
                  </span>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-medium">{o.name}</span>
                    <span className="block truncate text-xs text-white/45">
                      {t('org.members', { count: o.member_count })}
                    </span>
                  </span>
                </button>
              </li>
            ))
          )}
        </ul>
      </aside>

      {/* 右：組織の詳細（読み取り専用） */}
      <section ref={paneRef} className="min-h-0 flex-1 overflow-y-auto">
        {!detail ? (
          <div className="flex h-full flex-col items-center justify-center gap-1 text-center">
            <Building2 size={40} className="text-white/25" />
            <p className="text-sm text-white/45">{t('org.noSelection')}</p>
          </div>
        ) : (
          <div className="mx-auto max-w-xl p-6">
            <div className="mb-4 flex items-center gap-2">
              <Building2 size={22} className="shrink-0 text-white/50" />
              <h2 className="min-w-0 flex-1 truncate text-xl font-semibold">{detail.org.name}</h2>
              {onOpenContact && (
                <button
                  onClick={() => onOpenContact(detail.org.id)}
                  title={t('org.editInContacts')}
                  className="flex h-9 shrink-0 items-center gap-1.5 rounded-full bg-white/20 px-3.5 text-sm font-medium hover:bg-white/30"
                >
                  <Pencil size={15} />
                  {t('org.editInContacts')}
                </button>
              )}
            </div>

            {/* 代表の情報（代表電話・FAX・代表メール・URL・所在地）。メモは連絡先の画面で見る。 */}
            <div className="mb-5">
              <OrgCardInfo org={detail.org} />
            </div>

            {/* 共有アドレス（組織 ＋ 値 ＋ 共有件数） */}
            {detail && detail.shared_values.length > 0 && (
              <div className="mb-5">
                <span className="mb-1.5 flex items-center gap-1.5 text-xs text-white/50">
                  <Users size={14} />
                  {t('org.sharedValues')}
                </span>
                <ul className="space-y-1.5">
                  {detail.shared_values.map((v, i) => (
                    <li
                      key={`${v.kind}-${v.value}-${i}`}
                      className="flex items-center gap-2.5 rounded-md bg-white/5 px-3 py-2"
                    >
                      {v.kind === 'email' ? (
                        <Mail size={14} className="shrink-0 text-white/45" />
                      ) : (
                        <Phone size={14} className="shrink-0 text-white/45" />
                      )}
                      {v.label && (
                        <span className="shrink-0 rounded bg-white/10 px-1.5 py-0.5 text-[10px] text-white/60">
                          {v.label}
                        </span>
                      )}
                      <span className="min-w-0 flex-1 truncate text-sm">{v.value}</span>
                      <span className="flex shrink-0 items-center gap-1 rounded-full bg-amber-400/20 px-2 py-0.5 text-[11px] font-medium text-amber-200">
                        <Users size={11} />
                        {t('org.sharedCount', { count: v.count })}
                      </span>
                    </li>
                  ))}
                </ul>
              </div>
            )}

            {/* 所属連絡先 */}
            {detail && (
              <div className="mb-5">
                <span className="mb-1.5 flex items-center gap-1.5 text-xs text-white/50">
                  <User size={14} />
                  {t('org.membersLabel', { count: detail.members.length })}
                </span>
                {detail.members.length === 0 ? (
                  <p className="rounded-md bg-white/5 px-3 py-2 text-sm text-white/40">
                    {t('org.noMembers')}
                  </p>
                ) : (
                  <ul className="space-y-1">
                    {detail.members.map((m) => (
                      <li key={m.id}>
                        <button
                          onClick={() => onOpenContact?.(m.id)}
                          className="flex w-full items-center gap-2.5 rounded-md px-2.5 py-1.5 text-left hover:bg-white/10"
                        >
                          <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-white/15 text-xs font-semibold uppercase">
                            {m.display_name.trim().charAt(0) || <User size={13} />}
                          </span>
                          <span className="flex min-w-0 flex-1 items-center gap-1 truncate text-sm">
                            {m.is_favorite && (
                              <Gem size={11} className="shrink-0 fill-sky-300/30 text-sky-300" />
                            )}
                            {m.display_name}
                          </span>
                          {m.primary_email && (
                            <span className="shrink-0 truncate text-xs text-white/40">
                              {m.primary_email}
                            </span>
                          )}
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </div>
        )}
      </section>
    </div>
  );
}
