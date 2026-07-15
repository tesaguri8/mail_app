import { invoke } from '@tauri-apps/api/core';
import type { EventSummary } from '@bindings/EventSummary';
import type { EventInput } from '@bindings/EventInput';
import type { CalendarSummary } from '@bindings/CalendarSummary';
import type { CalendarInput } from '@bindings/CalendarInput';
import type { EventAttendee } from '@bindings/EventAttendee';
import type { AttendeeInput } from '@bindings/AttendeeInput';
import type { IcsImportReport } from '@bindings/IcsImportReport';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。

/** 期間 [from, to)（'YYYY-MM-DD' 等の ISO 文字列）に重なる予定を開始順で返す。 */
export const eventList = (from: string, to: string, includeDeleted = false) =>
  invoke<EventSummary[]>('event_list', { from, to, includeDeleted });

/** 論理削除済みの予定のみ（ゴミ箱一覧）。 */
export const eventListTrashed = () => invoke<EventSummary[]>('event_list_trashed');

/** タイトル・メモ・場所を横断して予定を検索（部分一致・期間非依存）。 */
export const eventSearch = (query: string, limit?: number) =>
  invoke<EventSummary[]>('event_search', { query, limit });

/** 場所欄のオートコンプリート候補（過去入力を頻度順）。 */
export const eventLocationSuggest = (query: string, limit?: number) =>
  invoke<string[]>('event_location_suggest', { query, limit });

/** タイトル欄のオートコンプリート候補（過去入力を頻度順）。 */
export const eventTitleSuggest = (query: string, limit?: number) =>
  invoke<string[]>('event_title_suggest', { query, limit });

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

// ── ゲスト（参加者） ──

/** 予定の参加者（ゲスト）一覧。 */
export const eventAttendeeList = (eventId: number) =>
  invoke<EventAttendee[]>('event_attendee_list', { eventId });

/** 予定の参加者を一括で置き換える。 */
export const eventAttendeeSet = (eventId: number, attendees: AttendeeInput[]) =>
  invoke<void>('event_attendee_set', { eventId, attendees });

// ── ICS 取込/書出（Google 互換） ──

/** .ics ファイルを取り込む。 */
export const icsImport = (path: string) => invoke<IcsImportReport>('ics_import', { path });

/** 全予定を .ics ファイルへ書き出す。 */
export const icsExport = (path: string) => invoke<void>('ics_export', { path });
