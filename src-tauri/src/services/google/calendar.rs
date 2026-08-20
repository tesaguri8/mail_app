//! Google カレンダー双方向同期（docs/CALENDAR_SYNC.md）。
//!
//! - `api`    : Google Calendar API v3 の薄い REST ラッパー。
//! - `convert`: Google の予定 ⇄ ローカル表現（EventSummary/events 行）の相互変換。
//! - `sync`   : 取り込み（pull）と送信（push）を束ねる同期エンジン。

pub mod api;
pub mod convert;
pub mod sync;

/// Google Calendar API v3 のベース URL。
pub const API_BASE: &str = "https://www.googleapis.com/calendar/v3";
