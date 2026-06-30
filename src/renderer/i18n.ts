import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import jaCommon from './locales/ja/common.json';
import enCommon from './locales/en/common.json';

export const SUPPORTED_LANGUAGES = ['ja', 'en'] as const;
export type SupportedLanguage = (typeof SUPPORTED_LANGUAGES)[number];

i18n.use(initReactI18next).init({
  resources: {
    ja: { common: jaCommon },
    en: { common: enCommon },
  },
  lng: 'ja',
  fallbackLng: 'en',
  ns: ['common'],
  defaultNS: 'common',
  interpolation: { escapeValue: false },
});

export default i18n;
