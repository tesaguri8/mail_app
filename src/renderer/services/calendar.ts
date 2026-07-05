import { invoke } from '@tauri-apps/api/core';
import type { EventSummary } from '@bindings/EventSummary';
import type { EventInput } from '@bindings/EventInput';
import type { CalendarSummary } from '@bindings/CalendarSummary';
import type { CalendarInput } from '@bindings/CalendarInput';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。

/** 期間 [from, to)（'YYYY-MM-DD' 等の ISO 文字列）に重なる予定を開始順で返す。 */
export const eventList = (from: string, to: string, includeDeleted = false) =>
  invoke<EventSummary[]>('event_list', { from, to, includeDeleted });

/** 論理削除済みの予定のみ（ゴミ箱一覧）。 */
export const eventListTrashed = () => invoke<EventSummary[]>('event_list_trashed');

/** 単一の予定を取得。 */
export const eventGet = (id: number) => invoke<EventSummary>('event_get', { id });

/** 予定を作成または更新（確定後の行を返す）。input.id が無ければ新規。 */
export const eventUpsert = (input: EventInput) => invoke<EventSummary>('event_upsert', { input });

/** 予定を論理削除（ゴミ箱へ。保持期間後に完全削除）。 */
export const eventDelete = (id: number) => invoke<void>('event_delete', { id });

/** 論理削除した予定を復元。 */
export const eventRestore = (id: number) => invoke<void>('event_restore', { id });

// ── カレンダー（マイ/他） ──

/** カレンダー一覧。 */
export const calendarList = () => invoke<CalendarSummary[]>('calendar_list');

/** カレンダーを作成または更新。 */
export const calendarUpsert = (input: CalendarInput) =>
  invoke<CalendarSummary>('calendar_upsert', { input });

/** カレンダーの表示オン/オフを切り替える。 */
export const calendarSetVisible = (id: number, visible: boolean) =>
  invoke<void>('calendar_set_visible', { id, visible });

/** カレンダーを削除（既定は不可。所属予定は既定へ付け替え）。削除できたら true。 */
export const calendarDelete = (id: number) => invoke<boolean>('calendar_delete', { id });
