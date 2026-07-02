import { invoke } from '@tauri-apps/api/core';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';

// メール作成の宛先オートコンプリート候補（住所録＋過去のやり取り相手）。
// docs/RECIPIENT_AUTOCOMPLETE.md
export const recipientSuggest = (query: string, limit = 8) =>
  invoke<RecipientSuggestion[]>('recipient_suggest', { query, limit });
