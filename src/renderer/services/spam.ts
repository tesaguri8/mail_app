import { invoke } from '@tauri-apps/api/core';
import type { SpamSettings } from '@bindings/SpamSettings';
import type { SpamVerdict } from '@bindings/SpamVerdict';
import type { SpamSenderConflict } from '@bindings/SpamSenderConflict';

// 迷惑メール設定の取得・保存（docs/SPAM.md §9）。DB を単一ソースにする。
export const spamSettingsGet = () => invoke<SpamSettings>('spam_settings_get');

export const spamSettingsSet = (settings: SpamSettings) =>
  invoke<void>('spam_settings_set', { settings });

// メール1件の迷惑スコアを算出し、判定（バンド・根拠トークン）を返す。
export const spamScore = (id: number) => invoke<SpamVerdict>('spam_score', { id });

// 迷惑差出人に登録済みだが、住所録/グリーンと矛盾する差出人の一覧（注意喚起用）。
export const spamFindConflicts = () =>
  invoke<SpamSenderConflict[]>('spam_find_conflicts');

// 指定アドレスを迷惑差出人から外し、同アドレスの隔離済みメールを受信箱へ戻す。
export const spamForgiveSender = (address: string) =>
  invoke<void>('spam_forgive_sender', { address });
