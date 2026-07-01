import { invoke } from '@tauri-apps/api/core';
import type { SpamSettings } from '@bindings/SpamSettings';
import type { SpamVerdict } from '@bindings/SpamVerdict';

// 迷惑メール設定の取得・保存（docs/SPAM.md §9）。DB を単一ソースにする。
export const spamSettingsGet = () => invoke<SpamSettings>('spam_settings_get');

export const spamSettingsSet = (settings: SpamSettings) =>
  invoke<void>('spam_settings_set', { settings });

// メール1件の迷惑スコアを算出し、判定（バンド・根拠トークン）を返す。
export const spamScore = (id: number) => invoke<SpamVerdict>('spam_score', { id });
