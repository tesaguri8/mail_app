import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { RefreshCcw } from 'lucide-react';
import type { ContactLink } from '@bindings/ContactLink';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { MergeRemoteDeletion } from '@bindings/MergeRemoteDeletion';
import { googleAccounts } from '../services/google';
import {
  contactGoogleDuplicatesOf,
  contactGoogleDuplicatesTidyOf,
  contactSyncStop,
  contactSyncTargetAdd,
} from '../services/contacts';
import { getNewContactTarget, setNewContactTarget } from '../config/prefs';
import { ConfirmDialog } from './ConfirmDialog';
import { GoogleDuplicateBar } from './GoogleDuplicateNotice';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 同期先に選べるアカウント（連携中＝解除中でない、かつ連絡先を同期している）。 */
export const selectableAccount = (a: GoogleAccount) => a.sync_contacts && a.disconnected_at == null;

/** 新規作成の画面で最初からチェックを入れるアカウント（選べるもののうち、前回外していないもの。
 *  未記録はオン）。`isOn` はアカウント別の前回の選択（既定は端末に覚えたもの）。 */
export const defaultTargets = (
  accounts: GoogleAccount[],
  isOn: (accountId: number) => boolean = getNewContactTarget
): Set<number> =>
  new Set(accounts.filter((a) => selectableAccount(a) && isOn(a.id)).map((a) => a.id));

/**
 * 連絡先の「同期先」（docs/CONTACT_MODEL.md §3「同期先は 1 人ずつ選ぶ」）。
 *
 * どこから追加しても、まず Rondine の連絡先として登録する。ここで選んだサービスにだけ保存する。
 * - 既存の連絡先（`contactId` あり）: チェックを入れると作成待ちを置く（次の同期で作る）。
 *   外すときは「向こうは残す（既定）／向こうも消す」を画面内で選ぶ
 * - 新規（`contactId` なし）: 選んだアカウントを `selected` に持ち、保存のあとに親が加える
 * 今は Google だけ。iCloud はアカウントの欄を並べる形で足せるようにしてある。
 */
export function ContactSyncTargets({
  contactId,
  links,
  selected,
  onSelectedChange,
  onChanged,
}: {
  contactId: number | null;
  links: ContactLink[];
  /** 新規のときの選択（保存後に加える）。null＝まだ決めていない（アカウントの既定で埋める）。 */
  selected: Set<number> | null;
  onSelectedChange: (s: Set<number>) => void;
  /** 既存の連絡先の同期先を変えた（つながりを読み直す）。 */
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [accounts, setAccounts] = useState<GoogleAccount[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // 外す確認（null＝出していない）と、向こうも消すか（既定は残す）。
  const [stopping, setStopping] = useState<{
    account: GoogleAccount;
    deleteRemote: boolean;
  } | null>(null);
  // 同じ Google アカウントに 2 件以上ある（以前の統合の名残）。アカウントごとの余りの件数。
  const [dupes, setDupes] = useState<MergeRemoteDeletion[]>([]);
  // 「1 件にまとめる」の確認（null＝出していない）。
  const [tidying, setTidying] = useState<MergeRemoteDeletion | null>(null);

  // つながりが変わるたびに数え直す（まとめたあと・同期先を変えたあと）。
  useEffect(() => {
    if (!isTauri || contactId === null) {
      setDupes([]);
      return;
    }
    contactGoogleDuplicatesOf(contactId)
      .then(setDupes)
      .catch(() => setDupes([]));
  }, [contactId, links]);

  useEffect(() => {
    if (!isTauri) return;
    googleAccounts()
      .then(setAccounts)
      .catch(() => setAccounts([]));
  }, []);

  // 新規の画面を開いたら、選べるアカウントに最初からチェックを入れる（前回外したものは外したまま）。
  useEffect(() => {
    if (contactId === null && selected === null && accounts.length > 0) {
      onSelectedChange(defaultTargets(accounts));
    }
  }, [contactId, selected, accounts, onSelectedChange]);

  const linkOf = (id: number) =>
    links.find((l) => l.provider === 'google' && l.account_id === id) ?? null;
  // 連絡先を同期しているアカウントと、（解除中などで選べなくても）つながりのあるアカウント。
  const shown = accounts.filter((a) => a.sync_contacts || linkOf(a.id) !== null);
  if (shown.length === 0) return null;

  const run = async (f: () => Promise<void>) => {
    setBusy(true);
    setError(null);
    try {
      await f();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (a: GoogleAccount) => {
    if (contactId === null) {
      const next = new Set(selected ?? []);
      if (next.has(a.id)) next.delete(a.id);
      else next.add(a.id);
      // 変えた選択は、次に新しく作るときの既定にする（メールからの追加でも同じ）。
      setNewContactTarget(a.id, next.has(a.id));
      onSelectedChange(next);
      return;
    }
    const link = linkOf(a.id);
    if (link === null) {
      void run(() => contactSyncTargetAdd(contactId, a.id));
    } else if (link.state === 'pending_create') {
      // まだ作っていないので、取り消すだけ（向こうには何も無い）。
      void run(() => contactSyncStop(contactId, a.id, false));
    } else {
      setStopping({ account: a, deleteRemote: false });
    }
  };

  // その人の重複だけを片付ける（1 つ残し、余りは次の同期で Google から削除。一括と同じ規則）。
  const tidy = () => {
    if (contactId === null) return;
    setTidying(null);
    void run(async () => {
      await contactGoogleDuplicatesTidyOf(contactId);
    });
  };

  const stop = () => {
    if (contactId === null || !stopping) return;
    const { account, deleteRemote } = stopping;
    setStopping(null);
    void run(() => contactSyncStop(contactId, account.id, deleteRemote));
  };

  return (
    <div>
      <span className="mb-1 flex items-center gap-1.5 text-[11px] text-white/50">
        <RefreshCcw size={14} />
        {t('contact.syncTargets')}
      </span>
      <ul className="space-y-1">
        {shown.map((a) => {
          const link = linkOf(a.id);
          const on =
            contactId === null
              ? (selected?.has(a.id) ?? false)
              : link !== null && link.state !== 'pending_delete';
          // 解除中・連絡先を同期していないアカウントは選べない。削除待ちは同期で消えるまで触らない。
          const locked = !selectableAccount(a) || link?.state === 'pending_delete';
          const note = !selectableAccount(a)
            ? t('contact.link.disconnected')
            : link?.state === 'pending_create'
              ? t('contact.syncPendingCreate')
              : link?.state === 'pending_delete'
                ? t('contact.syncPendingDelete')
                : null;
          return (
            <li key={a.id}>
              <label
                className={`flex items-center gap-2 rounded-md px-2 py-1.5 text-sm ${
                  locked ? 'opacity-50' : 'cursor-pointer hover:bg-white/5'
                }`}
              >
                <input
                  type="checkbox"
                  checked={on}
                  disabled={locked || busy}
                  onChange={() => toggle(a)}
                />
                <span className="min-w-0 flex-1 truncate">
                  {t('contact.syncSaveTo', { service: t('contact.link.google'), account: a.email })}
                </span>
                {note && (
                  <span className="shrink-0 rounded bg-amber-400/20 px-1.5 py-0.5 text-[10px] text-amber-200">
                    {note}
                  </span>
                )}
              </label>
            </li>
          );
        })}
      </ul>
      {dupes.map((d) => (
        <div key={d.account_id} className="mt-1">
          <GoogleDuplicateBar
            text={t('contact.syncDuplicates', { total: d.count + 1, account: d.account_label })}
            actionLabel={t('contact.syncDuplicatesRun')}
            onAction={() => setTidying(d)}
            disabled={busy}
          />
        </div>
      ))}
      <p className="mt-1 text-[11px] text-white/40">{t('contact.syncTargetsHint')}</p>
      {error && <p className="mt-1 text-xs text-red-300">{error}</p>}

      {tidying && (
        <ConfirmDialog
          title={t('contact.syncDuplicatesTitle')}
          body={t('contact.syncDuplicatesBody', {
            count: tidying.count,
            account: tidying.account_label,
          })}
          notes={[t('dupes.googleTrashNote')]}
          confirmLabel={t('contact.syncDuplicatesRun')}
          danger
          onConfirm={tidy}
          onCancel={() => setTidying(null)}
        />
      )}

      {stopping && (
        <ConfirmDialog
          title={t('contact.syncStopTitle', { account: stopping.account.email })}
          body={t('contact.syncStopBody')}
          notes={[
            stopping.deleteRemote ? t('contact.syncStopDeleteNote') : t('contact.syncStopKeepNote'),
          ]}
          confirmLabel={t('contact.syncStopRun')}
          danger={stopping.deleteRemote}
          onConfirm={stop}
          onCancel={() => setStopping(null)}
        >
          <div className="mt-3 space-y-1.5" role="radiogroup">
            {[false, true].map((deleteRemote) => (
              <label
                key={String(deleteRemote)}
                className={`flex cursor-pointer items-start gap-2 rounded-md px-2.5 py-2 text-sm ${
                  stopping.deleteRemote === deleteRemote ? 'bg-white/10' : 'hover:bg-white/5'
                }`}
              >
                <input
                  type="radio"
                  name="contact-sync-stop"
                  className="mt-1"
                  checked={stopping.deleteRemote === deleteRemote}
                  onChange={() => setStopping({ ...stopping, deleteRemote })}
                />
                <span className="text-white/90">
                  {deleteRemote ? t('contact.syncStopDelete') : t('contact.syncStopKeep')}
                </span>
              </label>
            ))}
          </div>
        </ConfirmDialog>
      )}
    </div>
  );
}
