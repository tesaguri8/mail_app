import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { RetentionReport } from '@bindings/RetentionReport';
import type { ServerAccountSummary } from '@bindings/ServerAccountSummary';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import type { StorageInfo } from '@bindings/StorageInfo';
import { useSync } from '../SyncProvider';
import { accountSetPassword, accountUpdate } from '../../services/accounts';
import {
  accountSetBodyWindow,
  accountSetFullWindow,
  accountSetStorageLimit,
  accountStorageInfo,
  mailRederiveAttachments,
  mailReprocess,
  rebuildPlan,
  storageOptimize,
} from '../../services/mail';
import { setLastSignature } from '../../config/prefs';
import { btnCls, inputCls, isTauri } from './shared';

// 添付ファイルを手元に残す期間: これより古いと添付ファイルをローカル削除。'all'=常に残す。
const FULL_WINDOWS = ['7d', '30d', '3m', '6m', '1y', 'all'] as const;
// テキスト全文を確実に残す期間（保証）: これより古い本文は容量オーバー時に要約対象。'all'=常に全文保持。
const BODY_WINDOWS = ['3m', '6m', '1y', '2y', 'all'] as const;

const GB = 1024 * 1024 * 1024;
const LIMIT_GB = [1, 2, 5, 10, 20, 50] as const;

function formatBytes(b: number): string {
  if (b < 1024 * 1024) return `${Math.round(b / 1024)} KB`;
  if (b < GB) return `${(b / 1024 / 1024).toFixed(0)} MB`;
  return `${(b / GB).toFixed(2)} GB`;
}

/** 保持期間ウィンドウ値の表示ラベル（'all'=常に保持 / それ以外は期間）。 */
function windowLabel(t: (k: string) => string, w: string): string {
  if (w === 'all') return t('storage.keepAll');
  return t(`mailbox.w_${w}`);
}

/**
 * カードの「メール」の詳細: 差出人名・既定署名・パスワードの入れ直し・容量と保持期間・
 * サーバー設定（共有先）・再構築。メール固有の設定はここに置く（docs/ACCOUNTS.md §2-1）。
 */
export function MailAccountDetails({
  account,
  accounts,
  servers,
  signatures,
  onChanged,
}: {
  account: AccountSummary;
  /** 全メールアカウント（サーバー設定の共有先のアドレスを出すため）。 */
  accounts: AccountSummary[];
  servers: ServerAccountSummary[];
  signatures: SignatureSummary[];
  onChanged: () => void;
}) {
  const { t } = useTranslation();
  const [editName, setEditName] = useState(account.display_name ?? '');
  const [editSig, setEditSig] = useState<number | null>(account.signature_id ?? null);
  // 資格情報が失われたときに入れ直すためのパスワード。
  const [editPassword, setEditPassword] = useState('');
  const [editFullWindow, setEditFullWindow] = useState(account.full_window ?? 'all');
  const [editBodyWindow, setEditBodyWindow] = useState(account.body_window ?? 'all');
  const [editStatus, setEditStatus] = useState('');
  // ストレージ（容量）状態
  const [storage, setStorage] = useState<StorageInfo | null>(null);
  const [storageBusy, setStorageBusy] = useState(false);
  const [storageMsg, setStorageMsg] = useState('');
  // 同期/再取り込みはアプリ全体のバックグラウンド実行（進捗・中断・完了トースト）に委譲。
  const sync = useSync();

  const server = servers.find((s) => s.account_ids.includes(account.id));
  const sharedWith = (server?.account_ids ?? [])
    .filter((id) => id !== account.id)
    .map((id) => accounts.find((a) => a.id === id)?.email)
    .filter((e): e is string => !!e);

  const loadStorage = (id: number) => {
    if (!isTauri) return;
    accountStorageInfo(id)
      .then(setStorage)
      .catch(() => undefined);
  };
  useEffect(() => loadStorage(account.id), [account.id]);

  const changeLimit = async (id: number, gb: number) => {
    setStorage((s) => (s ? { ...s, limit_bytes: gb * GB } : s));
    try {
      await accountSetStorageLimit(id, gb * GB);
      await storageOptimize(id); // 新上限で超過分があれば即整理
      loadStorage(id);
    } catch (e) {
      setStorageMsg(String(e));
    }
  };

  // 保持レポート（削除した添付・要約した本文・解放量）を1行のメッセージにする。
  const retentionMsg = (r: RetentionReport): string =>
    t('storage.optimized', {
      count: r.evicted,
      compacted: r.compacted,
      size: formatBytes(r.freed_bytes),
    });

  const optimize = async (id: number) => {
    setStorageBusy(true);
    setStorageMsg('');
    try {
      const r = await storageOptimize(id);
      setStorageMsg(retentionMsg(r));
      loadStorage(id);
    } catch (e) {
      setStorageMsg(String(e));
    } finally {
      setStorageBusy(false);
    }
  };

  // 再構築: データ形式バージョンで判定し、サーバーからの全体再取り込みが必要なときだけ
  // 再取り込み（バックグラウンド・進捗/中断は共通インジケータ）、それ以外はローカル再解析
  // （保存済み本文から引用分離・スレッド束ねを作り直し。通信なし）を実行する。
  const [rebuilding, setRebuilding] = useState<number | null>(null);
  const startRebuild = async (id: number, email: string) => {
    setRebuilding(id);
    try {
      const plan = await rebuildPlan(id);
      if (plan.action === 'resync') {
        sync.toast(t('storage.rebuildResync'));
        sync.start(id, email, 'resync');
      } else {
        const n = await mailReprocess(id);
        sync.toast(t('storage.reprocessed', { count: n }));
        onChanged();
      }
    } catch (e) {
      sync.toast(String(e), 'error');
    } finally {
      setRebuilding(null);
    }
  };

  // 開発用: 添付本体を落とさず BODYSTRUCTURE だけ取り直し、添付メタを section 付きで作り直す。
  // ネスト添付の取りこぼし修正＆開発DBの掃除（再ダウンロードなしで軽い）。
  const [rederiving, setRederiving] = useState<number | null>(null);
  const startRederive = async (id: number) => {
    setRederiving(id);
    try {
      const n = await mailRederiveAttachments(id);
      sync.toast(`添付メタを再導出しました（${n} 件）`);
      onChanged();
    } catch (e) {
      sync.toast(String(e), 'error');
    } finally {
      setRederiving(null);
    }
  };

  // パスワードだけを入れ直す（資格情報が失われたときの復旧。メールは消さない）。
  const savePassword = async (id: number) => {
    if (!editPassword) return;
    try {
      await accountSetPassword(id, editPassword);
      setEditPassword('');
      setEditStatus('✓ ' + t('account.passwordSaved'));
    } catch (e) {
      setEditStatus('✕ ' + String(e));
    }
  };

  const saveEdit = async () => {
    const id = account.id;
    try {
      await accountUpdate(id, editName.trim() || null, editSig);
      // 作成画面は「最後に使った署名」を優先するので、ここで既定を変えたらそれも更新する
      // （設定で選び直したのに次の作成画面へ反映されない、を防ぐ）。
      setLastSignature(id, editSig);
      setEditStatus('✓ ' + t('account.saved'));
      onChanged();
    } catch (e) {
      setEditStatus('✕ ' + String(e));
    }
  };

  // 添付を手元に残す期間の変更（設定後すぐ保持ポリシーを適用し、結果を表示）。
  const changeFullWindow = async (id: number, w: string) => {
    setEditFullWindow(w);
    setStorageMsg('');
    try {
      const r = await accountSetFullWindow(id, w);
      setStorageMsg(retentionMsg(r));
      loadStorage(id);
      onChanged();
    } catch (e) {
      setStorageMsg(String(e));
    }
  };

  // 本文の全文保持期間の変更（設定後すぐ要約保存を適用し、結果を表示）。
  const changeBodyWindow = async (id: number, w: string) => {
    setEditBodyWindow(w);
    setStorageMsg('');
    try {
      const r = await accountSetBodyWindow(id, w);
      setStorageMsg(retentionMsg(r));
      loadStorage(id);
      onChanged();
    } catch (e) {
      setStorageMsg(String(e));
    }
  };

  return (
    <div className="space-y-3 border-t border-white/10 bg-black/15 px-3 py-3">
      <label className="block">
        <span className="mb-1 block text-xs text-white/55">{t('account.displayName')}</span>
        <input
          className={inputCls}
          placeholder={t('account.displayNamePlaceholder')}
          value={editName}
          onChange={(e) => setEditName(e.target.value)}
        />
      </label>
      <label className="block">
        <span className="mb-1 block text-xs text-white/55">{t('account.signature')}</span>
        <select
          className={inputCls}
          value={editSig ?? ''}
          onChange={(e) => setEditSig(e.target.value === '' ? null : Number(e.target.value))}
        >
          <option value="">{t('account.signatureNone')}</option>
          {signatures.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name || t('signature.untitled')}
            </option>
          ))}
        </select>
      </label>
      {/* パスワードの入れ直し（資格情報が失われたときの復旧手段） */}
      <div className="rounded-md border border-white/10 p-3">
        <div className="mb-1 text-xs text-white/55">{t('account.resetPassword')}</div>
        <p className="mb-2 text-[11px] text-white/40">{t('account.resetPasswordHint')}</p>
        <div className="flex gap-2">
          <input
            className={inputCls}
            type="password"
            placeholder={t('account.password')}
            value={editPassword}
            onChange={(e) => setEditPassword(e.target.value)}
          />
          <button
            className={btnCls}
            onClick={() => savePassword(account.id)}
            disabled={!editPassword}
          >
            {t('account.save')}
          </button>
        </div>
      </div>
      {/* ストレージ（容量上限とエビクション） */}
      <div className="rounded-md border border-white/10 p-3">
        <div className="mb-1 flex items-center justify-between">
          <span className="text-xs text-white/55">{t('storage.title')}</span>
          <span className="text-xs text-white/70">
            {storage ? (
              <>
                {formatBytes(storage.used_bytes)} /{' '}
                <span
                  className={
                    storage.used_bytes > storage.limit_bytes * 1.05
                      ? 'font-semibold text-red-400'
                      : ''
                  }
                  title={
                    storage.used_bytes > storage.limit_bytes * 1.05
                      ? t('storage.overLimit')
                      : undefined
                  }
                >
                  {formatBytes(storage.limit_bytes)}
                </span>
              </>
            ) : (
              '—'
            )}
          </span>
        </div>
        {storage && (
          <div className="mb-2 h-1.5 overflow-hidden rounded-full bg-white/10">
            <div
              className={`h-full rounded-full ${
                storage.used_bytes > storage.limit_bytes * 1.05 ? 'bg-red-400' : 'bg-sky-400'
              }`}
              style={{
                width: `${Math.min(100, storage.limit_bytes > 0 ? (storage.used_bytes / storage.limit_bytes) * 100 : 0)}%`,
              }}
            />
          </div>
        )}
        {/* 期間ベースの3ティア: フルデータ → 添付削除 → 本文要約 */}
        <label className="mb-2 block">
          <span className="mb-1 block text-xs text-white/55">{t('storage.fullWindow')}</span>
          <select
            className={inputCls}
            value={editFullWindow}
            onChange={(e) => changeFullWindow(account.id, e.target.value)}
          >
            {FULL_WINDOWS.map((w) => (
              <option key={w} value={w}>
                {windowLabel(t, w)}
              </option>
            ))}
          </select>
          <span className="mt-1 block text-[11px] leading-snug text-white/40">
            {t('storage.fullWindowHint')}
          </span>
        </label>
        <label className="mb-2 block">
          <span className="mb-1 block text-xs text-white/55">{t('storage.bodyWindow')}</span>
          <select
            className={inputCls}
            value={
              (BODY_WINDOWS as readonly string[]).includes(editBodyWindow)
                ? editBodyWindow
                : 'custom'
            }
            onChange={(e) => {
              if (e.target.value === 'custom') {
                // カスタムに切替: 既にプリセット外のカスタム年数ならそのまま、
                // そうでなければ（プリセット '1y'/'2y' 等からの切替も含め）既定 3 年。
                const isPreset = (BODY_WINDOWS as readonly string[]).includes(editBodyWindow);
                changeBodyWindow(
                  account.id,
                  !isPreset && /^\d+y$/.test(editBodyWindow) ? editBodyWindow : '3y'
                );
              } else {
                changeBodyWindow(account.id, e.target.value);
              }
            }}
          >
            {BODY_WINDOWS.map((w) => (
              <option key={w} value={w}>
                {windowLabel(t, w)}
              </option>
            ))}
            <option value="custom">{t('storage.bodyWindowCustom')}</option>
          </select>
          {!(BODY_WINDOWS as readonly string[]).includes(editBodyWindow) && (
            <div className="mt-2 flex items-center gap-2">
              <input
                type="number"
                min={1}
                max={99}
                key={editBodyWindow}
                defaultValue={/^(\d+)y$/.exec(editBodyWindow)?.[1] ?? '3'}
                onBlur={(e) => {
                  const n = Math.max(1, Math.min(99, Math.floor(Number(e.target.value) || 1)));
                  changeBodyWindow(account.id, `${n}y`);
                }}
                className="w-20 rounded-md bg-white/10 px-2 py-1.5 text-sm text-white outline-none focus:bg-white/20"
              />
              <span className="text-xs text-white/55">{t('storage.years')}</span>
            </div>
          )}
          <span className="mt-1 block text-[11px] leading-snug text-white/40">
            {t('storage.bodyWindowHint')}
          </span>
        </label>
        <label className="mb-2 block">
          <span className="mb-1 block text-xs text-white/55">{t('storage.limit')}</span>
          <select
            className={inputCls}
            value={storage ? Math.round(storage.limit_bytes / GB) : 2}
            onChange={(e) => changeLimit(account.id, Number(e.target.value))}
          >
            {LIMIT_GB.map((g) => (
              <option key={g} value={g}>
                {g} GB
              </option>
            ))}
          </select>
        </label>
        <div className="flex flex-wrap items-center gap-2">
          <button className={btnCls} disabled={storageBusy} onClick={() => optimize(account.id)}>
            {t('storage.optimize')}
          </button>
          {storageMsg && <span className="text-xs text-white/70">{storageMsg}</span>}
        </div>
      </div>

      {/* サーバー設定（ほかのアドレスと共有していれば共有先も見せる。docs/ACCOUNTS.md §4） */}
      {server && (
        <div className="rounded-md border border-white/10 p-3 text-xs">
          <div className="mb-1 text-white/55">{t('account.serverAccount')}</div>
          <div className="text-white/75">
            IMAP {server.imap_host}:{server.imap_port} · SMTP {server.smtp_host}:{server.smtp_port}
          </div>
          <div className="text-white/45">{t('account.loginName', { name: server.username })}</div>
          {sharedWith.length > 0 && (
            <div className="mt-1 text-white/45">
              {t('account.serverSharedWith', { emails: sharedWith.join(', ') })}
            </div>
          )}
        </div>
      )}

      <div className="flex items-center gap-3">
        <button className={btnCls} onClick={() => void saveEdit()}>
          {t('account.save')}
        </button>
        {editStatus && <span className="text-xs text-white/70">{editStatus}</span>}
      </div>

      {/* 再構築: データ形式バージョンで全体再取り込み／ローカル再解析を自動選択 */}
      <div className="mt-1 border-t border-white/10 pt-3">
        {sync.active?.accountId === account.id ? (
          <button className={btnCls} onClick={sync.cancel} title={t('storage.rebuildHint')}>
            {t('sync.cancel')}
          </button>
        ) : (
          <button
            className={btnCls}
            disabled={rebuilding === account.id || !!sync.active}
            onClick={() => startRebuild(account.id, account.email)}
            title={t('storage.rebuildHint')}
          >
            {rebuilding === account.id ? t('storage.rebuilding') : t('storage.rebuild')}
          </button>
        )}
        <span className="mt-1 block text-[11px] leading-snug text-white/40">
          {t('storage.rebuildHint')}
        </span>
        {/* 開発用: 添付メタ再導出（BODYSTRUCTURE のみ・本体を落とさない） */}
        {import.meta.env.DEV && (
          <div className="mt-2">
            <button
              className={btnCls}
              disabled={rederiving === account.id || !!sync.active}
              onClick={() => startRederive(account.id)}
              title="添付本体を落とさず BODYSTRUCTURE だけ取り直し、添付メタを section 付きで作り直します（開発用）"
            >
              {rederiving === account.id ? '添付メタを再導出中…' : '添付メタを再導出（dev）'}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
