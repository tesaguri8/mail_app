import { useEffect, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Plus } from 'lucide-react';
import type { AccountProfile } from '@bindings/AccountProfile';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import type { GoogleCredentialsStatus } from '@bindings/GoogleCredentialsStatus';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import {
  accountCheck,
  accountPing,
  accountProfileReorder,
  accountProfiles,
  serverAccountList,
} from '../../services/accounts';
import { googleAccounts, googleCredentialsStatus } from '../../services/google';
import { signatureList } from '../../services/signatures';
import { AccountCard } from './AccountCard';
import { AddAccountFlow } from './AddAccountFlow';
import { GoogleCredentialsPanel } from './GoogleCredentialsPanel';
import { type ConnState, isTauri, sameAddress } from './shared';

/**
 * 設定の「アカウント」（docs/ACCOUNTS.md）。アドレスごとに 1 枚のカードを並べ、その中で
 * メール・連絡先・カレンダーを選ぶ。以前の「同期」メニューの中身もカードの中へ移した。
 */
export function AccountsSettings({
  accounts,
  onChanged,
}: {
  /** メールアカウント（アプリ全体で持っている一覧。件数などの更新で新しい配列になる）。 */
  accounts: AccountSummary[];
  /** メールアカウントが変わったとき（アプリ全体の一覧を読み直す）。 */
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [profiles, setProfiles] = useState<AccountProfile[]>([]);
  const [googles, setGoogles] = useState<GoogleAccount[]>([]);
  const [servers, setServers] = useState<ServerAccountSummary[]>([]);
  const [signatures, setSignatures] = useState<SignatureSummary[]>([]);
  const [creds, setCreds] = useState<GoogleCredentialsStatus | null>(null);
  const [adding, setAdding] = useState(false);
  const [conn, setConn] = useState<Record<number, ConnState>>({});
  // 開いているカード（同時に 1 枚だけ。最初は全部閉じる＝null）。
  const [openId, setOpenId] = useState<number | null>(null);
  // 追加の流れで作ったカードのアドレス。一覧に現れたら、そのカードを開く。
  const [created, setCreated] = useState<string | null>(null);
  useEffect(() => {
    if (!created) return;
    const p = profiles.find((x) => sameAddress(x.email, created));
    if (!p) return;
    setOpenId(p.id);
    setCreated(null);
  }, [created, profiles]);

  const load = () => {
    if (!isTauri) return;
    accountProfiles()
      .then(setProfiles)
      .catch(() => undefined);
    googleAccounts()
      .then(setGoogles)
      .catch(() => setGoogles([]));
    serverAccountList()
      .then(setServers)
      .catch(() => undefined);
    googleCredentialsStatus()
      .then(setCreds)
      .catch(() => undefined);
  };
  // メールアカウントの増減（追加・外す）でもカードが変わるので、一覧が変わるたびに読み直す。
  const mailIds = accounts.map((a) => a.id).join(',');
  useEffect(load, [mailIds]);
  useEffect(() => {
    if (!isTauri) return;
    signatureList()
      .then(setSignatures)
      .catch(() => undefined);
  }, []);

  const changed = () => {
    onChanged();
    load();
  };

  // 接続の点のチェック。既定は軽量な TCP 到達確認（速い・固まらない・連続ログインにならない）。
  // full=true（点のクリック）のときだけ、資格情報での実 LOGIN で厳密に確認する。
  const checkConn = (id: number, full = false) => {
    setConn((c) => ({ ...c, [id]: { state: 'checking' } }));
    return (full ? accountCheck(id) : accountPing(id))
      .then(() => setConn((c) => ({ ...c, [id]: { state: 'ok' } })))
      .catch((e) => setConn((c) => ({ ...c, [id]: { state: 'error', msg: String(e) } })));
  };
  // 各アカウントの接続状態は「初めて見た時に1回だけ」チェックする。accounts は同期で件数が
  // 更新されるたびに新配列になるため、そのたびにログインを投げるとサーバーに連続ログインと
  // みなされ遮断される（さくら等）。手動チェックは点のクリックで随時できる。
  const checkedRef = useRef<Set<number>>(new Set());
  useEffect(() => {
    if (!isTauri) return;
    const toCheck = accounts.filter((a) => !checkedRef.current.has(a.id));
    if (toCheck.length === 0) return;
    toCheck.forEach((a) => checkedRef.current.add(a.id));
    // 同時に複数ログインを開かず、1件ずつ順番に確認する（サーバーの多重接続制限に配慮）。
    void (async () => {
      for (const a of toCheck) await checkConn(a.id);
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accounts]);

  // ドラッグ&ドロップの並べ替え（カード単位。メールの一覧の並びもカードの順にそろう）。
  const dragId = useRef<number | null>(null);
  // つまみを押したカードだけを draggable にする（カード全体を常に draggable にすると、
  // 中の入力欄で文字を選べなくなる）。
  const [armed, setArmed] = useState<number | null>(null);
  const [dragOverId, setDragOverId] = useState<number | null>(null);
  const reorder = (toId: number) => {
    const fromId = dragId.current;
    dragId.current = null;
    setDragOverId(null);
    setArmed(null);
    if (fromId == null || fromId === toId) return;
    const from = profiles.findIndex((p) => p.id === fromId);
    const to = profiles.findIndex((p) => p.id === toId);
    if (from < 0 || to < 0) return;
    const next = [...profiles];
    const [moved] = next.splice(from, 1);
    next.splice(to, 0, moved);
    setProfiles(next);
    accountProfileReorder(next.map((p) => p.id))
      .then(onChanged)
      .catch(() => undefined);
  };

  return (
    <div className="max-w-xl space-y-3 text-left">
      <div>
        <div className="text-base font-semibold text-white">{t('settings.accounts')}</div>
        <p className="mt-0.5 text-xs text-white/45">{t('account.intro')}</p>
      </div>

      {profiles.length === 0 && !adding && (
        <p className="text-sm text-white/60">{t('account.none')}</p>
      )}

      {profiles.map((p) => (
        <div
          key={p.id}
          draggable={armed === p.id}
          onMouseUp={() => setArmed(null)}
          onDragStart={() => {
            dragId.current = p.id;
          }}
          onDragOver={(e) => {
            if (dragId.current == null) return;
            e.preventDefault();
            if (dragOverId !== p.id) setDragOverId(p.id);
          }}
          onDrop={(e) => {
            e.preventDefault();
            reorder(p.id);
          }}
          onDragEnd={() => {
            dragId.current = null;
            setDragOverId(null);
            setArmed(null);
          }}
          className={`rounded-lg ${dragOverId === p.id ? 'ring-1 ring-sky-300/60' : ''}`}
        >
          <AccountCard
            profile={p}
            mails={p.mail_account_ids
              .map((id) => accounts.find((a) => a.id === id))
              .filter((a): a is AccountSummary => !!a)}
            allAccounts={accounts}
            google={googles.find((g) => g.id === p.google_account_id)}
            creds={creds}
            servers={servers}
            signatures={signatures}
            conn={conn}
            onCheckConn={(id) => void checkConn(id, true)}
            onChanged={changed}
            open={openId === p.id}
            onToggleOpen={() => setOpenId(openId === p.id ? null : p.id)}
            // 追加の途中は並べ替えない。
            onDragHandleDown={profiles.length > 1 && !adding ? () => setArmed(p.id) : undefined}
          />
        </div>
      ))}

      {adding ? (
        <AddAccountFlow
          profiles={profiles}
          servers={servers}
          creds={creds}
          onChanged={changed}
          onCreated={setCreated}
          onClose={() => setAdding(false)}
        />
      ) : (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-1.5 rounded-md border border-white/20 bg-black/40 px-3 py-2 text-sm text-white/80 backdrop-blur-sm hover:bg-black/55 hover:text-white"
        >
          <Plus size={16} />
          {t('account.addAccount')}
        </button>
      )}

      <div className="pt-4">
        <GoogleCredentialsPanel creds={creds} onSaved={load} />
      </div>
      {!isTauri && <p className="text-xs text-white/40">{t('settings.spamPreviewNote')}</p>}
    </div>
  );
}
