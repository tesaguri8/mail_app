import { useTranslation } from 'react-i18next';

/**
 * 未実装ビューのプレースホルダ（住所録・カレンダー等）。
 */
export function StubView({ titleKey }: { titleKey: string }) {
  const { t } = useTranslation();
  return (
    <div className="flex h-full flex-col items-center justify-center gap-2 text-center">
      <p className="text-lg font-medium text-white/80">{t(titleKey)}</p>
      <p className="text-sm text-white/45">{t('comingSoon')}</p>
    </div>
  );
}
