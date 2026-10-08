import { useTranslation } from 'react-i18next';
import { ArrowUpToLine, Briefcase, Building2, Plus, X } from 'lucide-react';
import type { ContactOrganization } from '@bindings/ContactOrganization';
import { Field } from './ContactValueEditor';
import { OrgCombobox } from './OrgCombobox';
import { emptyOrganization } from '../utils/contactDraft';

const INPUT = 'w-full rounded bg-white/10 px-2.5 py-1.5 text-sm outline-none focus:bg-white/15';

// 空文字は NULL に寄せる（検索・並び替え・送信の一貫性のため）。
const nullify = (s: string) => (s.trim() === '' ? null : s);

/**
 * 所属する会社の複数編集（先頭が主）。各会社は組織カードを選ぶか会社名を入れ、よみ・役職・部署を持つ。
 * `savedNames` はこの連絡先に保存済みの会社名（小文字・前後空白なし）。保存済みの名前は保存しても
 * 組織カードにならない（docs/CONTACT_MODEL.md §8）ので、欄の注記を変える。
 * `children` は主の会社の直下に出すもの（組織カードの表示・重なりの通知）。
 * 会社名のよみは普段使わないので、`showPhonetic` か値があるときだけ出す。
 */
export function OrgRows({
  organizations,
  onChange,
  savedNames,
  showPhonetic,
  children,
}: {
  organizations: ContactOrganization[];
  onChange: (orgs: ContactOrganization[]) => void;
  savedNames: ReadonlySet<string>;
  showPhonetic: boolean;
  children?: React.ReactNode;
}) {
  const { t } = useTranslation();
  // 1 社も無いときも、主の会社の欄は出しておく（よく使う項目なので畳まない）。
  const rows = organizations.length > 0 ? organizations : [emptyOrganization()];
  const set = (i: number, patch: Partial<ContactOrganization>) =>
    onChange(rows.map((o, idx) => (idx === i ? { ...o, ...patch } : o)));
  const remove = (i: number) => onChange(rows.filter((_, idx) => idx !== i));
  const makePrimary = (i: number) => onChange([rows[i], ...rows.filter((_, idx) => idx !== i)]);
  const many = rows.length > 1;

  return (
    <div className="space-y-3">
      {rows.map((o, i) => (
        <div
          key={i}
          className={
            many ? 'space-y-2 rounded-md border border-white/10 bg-white/5 p-2' : 'space-y-3'
          }
        >
          {many && (
            <div className="flex items-center gap-2 text-[11px] text-white/50">
              <span className="flex-1">
                {i === 0 ? t('contact.orgPrimary') : t('contact.orgNth', { n: i + 1 })}
              </span>
              {i > 0 && (
                <button
                  onClick={() => makePrimary(i)}
                  title={t('contact.orgMakePrimary')}
                  aria-label={t('contact.orgMakePrimary')}
                  className="flex h-6 w-6 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
                >
                  <ArrowUpToLine size={13} />
                </button>
              )}
              <button
                onClick={() => remove(i)}
                title={t('contact.removeRow')}
                aria-label={t('contact.removeRow')}
                className="flex h-6 w-6 items-center justify-center rounded-full text-white/40 hover:bg-white/10 hover:text-white"
              >
                <X size={13} />
              </button>
            </div>
          )}
          <OrgCombobox
            orgId={o.org_id}
            name={o.name ?? ''}
            saved={savedNames.has((o.name ?? '').trim().toLowerCase())}
            onChange={(org_id, name) => set(i, { org_id, name: nullify(name) })}
          />
          {i === 0 && children}
          {(showPhonetic || o.phonetic_name) && (
            <Field icon={<Building2 size={15} />} label={t('contact.orgPhonetic')}>
              <input
                className={INPUT}
                value={o.phonetic_name ?? ''}
                onChange={(e) => set(i, { phonetic_name: nullify(e.target.value) })}
              />
            </Field>
          )}
          <div className="grid grid-cols-2 gap-2">
            <Field icon={<Briefcase size={15} />} label={t('contact.orgTitle')}>
              <input
                className={INPUT}
                value={o.title ?? ''}
                onChange={(e) => set(i, { title: nullify(e.target.value) })}
              />
            </Field>
            <Field icon={<Building2 size={15} />} label={t('contact.orgDepartment')}>
              <input
                className={INPUT}
                value={o.department ?? ''}
                onChange={(e) => set(i, { department: nullify(e.target.value) })}
              />
            </Field>
          </div>
        </div>
      ))}
      <button
        onClick={() => onChange([...rows, emptyOrganization()])}
        className="flex items-center gap-1 text-xs text-sky-300 hover:text-sky-200"
      >
        <Plus size={13} />
        {t('contact.addOrganization')}
      </button>
    </div>
  );
}
