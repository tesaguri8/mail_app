import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import jaCommon from './locales/ja/common.json';
import enCommon from './locales/en/common.json';

export const SUPPORTED_LANGUAGES = ['ja', 'en'] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

const LANG_KEY = 'rondine.lang';

/** 前回選択した言語を復元（未保存/不正なら既定の日本語）。 */
function initialLanguage(): SupportedLanguage {
  try {
    const saved = localStorage.getItem(LANG_KEY);
    if (saved && (SUPPORTED_LANGUAGES as readonly string[]).includes(saved)) {
      return saved as SupportedLanguage;
    }
  } catch {
    /* localStorage を使えない環境では既定にフォールバック */
  }
  return 'ja';
}

i18n.use(initReactI18next).init({
  resources: {
    ja: { common: jaCommon },
    en: { common: enCommon },
  },
  lng: initialLanguage(),
  fallbackLng: 'en',
  ns: ['common'],
  defaultNS: 'common',
  interpolation: { escapeValue: false },
});

// 言語切替（タイトルバーのトグル等）を保存して次回起動時に復元する。
i18n.on('languageChanged', (lng) => {
  try {
    localStorage.setItem(LANG_KEY, lng);
  } catch {
    /* noop */
  }
});

export default i18n;
