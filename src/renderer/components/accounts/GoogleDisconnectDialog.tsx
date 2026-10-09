import { useTranslation } from 'react-i18next';
import type { GoogleAccount } from '@bindings/GoogleAccount';
import { ConfirmDialog } from '../ConfirmDialog';

/**
 * Google 連携の解除の確認。「一時的に解除（記録を残す・既定）」か「完全に解除」かを選ぶ。
 * 解除中のアカウントは、もう一時的な解除を選べない。
 */
export function GoogleDisconnectDialog({
  account,
  purge,
  busy,
  onPurgeChange,
  onConfirm,
  onCancel,
}: {
  account: GoogleAccount;
  purge: boolean;
  busy: boolean;
  onPurgeChange: (purge: boolean) => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const { t } = useTranslation();
  return (
    <ConfirmDialog
      title={t('settings.gcalDisconnectTitle', { email: account.email })}
      body={t('settings.gcalDisconnectBody')}
      notes={[purge ? t('settings.gcalPurgeNote') : t('settings.gcalDisconnectKeepNote')]}
      confirmLabel={purge ? t('settings.gcalPurgeRun') : t('settings.gcalDisconnectRun')}
      danger={purge}
      busy={busy}
      onConfirm={onConfirm}
      onCancel={onCancel}
    >
      <div className="mt-3 space-y-1.5" role="radiogroup">
        {[false, true].map((option) => {
          const unavailable = !option && account.disconnected_at != null;
          return (
            <label
              key={String(option)}
              className={`flex items-start gap-2 rounded-md px-2.5 py-2 text-sm ${
                purge === option ? 'bg-white/10' : 'hover:bg-white/5'
              } ${unavailable ? 'opacity-40' : 'cursor-pointer'}`}
            >
              <input
                type="radio"
                name="gcal-disconnect-kind"
                className="mt-1"
                checked={purge === option}
                disabled={unavailable}
                onChange={() => onPurgeChange(option)}
              />
              <span>
                <span className="block text-white/90">
                  {option ? t('settings.gcalPurgeOption') : t('settings.gcalDisconnectOption')}
                </span>
                <span className="block text-xs text-white/45">
                  {option
                    ? t('settings.gcalPurgeOptionHint')
                    : t('settings.gcalDisconnectOptionHint')}
                </span>
              </span>
            </label>
          );
        })}
      </div>
    </ConfirmDialog>
  );
}
