//! アプリのログ（tauri-plugin-log）の設定。
//!
//! 書き出し先は既定のまま（標準出力とログフォルダの `<製品名>.log`）。ここで決めるのは、
//! 1 ファイルの大きさ・残す世代・どの水準まで残すか。
//!
//! 水準を絞らないと、reqwest の TRACE や keyring の DEBUG まで全部ファイルに入り、
//! 同期の経緯（IMAP の接続 1 回ごとの行など）が埋もれ、回転も早まる（2026-11-11 vaio）。

use log::LevelFilter;
use tauri::{Manager, Runtime};
use tauri_plugin_log::{Builder, RotationStrategy};

/// ログの 1 ファイルの上限（バイト）。
pub const FILE_BYTES: u128 = 5 * 1024 * 1024;
/// ログを残す世代の数（いま書いているファイルを含む）。
pub const FILES: usize = 5;
/// 既定で残す水準。依存クレートの DEBUG・TRACE（reqwest・hyper・keyring・tao など）はここで落ちる。
pub const DEFAULT_LEVEL: LevelFilter = LevelFilter::Info;
/// target（モジュールパス）の前方一致で、既定と違う水準にするもの。
/// 自分のクレートは、既定を上げても INFO（IMAP の接続ごとの行など）を必ず残す。
pub const LEVELS: &[(&str, LevelFilter)] = &[("rondine_lib", LevelFilter::Info)];

/// ログのプラグインを組み立てる（`tauri::Builder::plugin` に渡す）。
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    LEVELS
        .iter()
        .fold(
            Builder::default().level(DEFAULT_LEVEL),
            |b, (target, level)| b.level_for(*target, *level),
        )
        .max_file_size(FILE_BYTES)
        .rotation_strategy(RotationStrategy::KeepSome(FILES))
        .build()
}

/// 起動の 1 行を残す（版・pid・ログの置き場）。
///
/// どの版が書いたログか、同じファイルに別のプロセスが書いていないかを、後から読み分けるため。
pub fn log_startup<R: Runtime>(app: &tauri::AppHandle<R>) {
    let dir = app
        .path()
        .app_log_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|e| format!("不明（{e}）"));
    log::info!(
        "{} {} を起動（pid {}・ログ {dir}・{} MB × {FILES} 世代・{DEFAULT_LEVEL} 以上）",
        app.package_info().name,
        app.package_info().version,
        std::process::id(),
        FILE_BYTES / (1024 * 1024),
    );
}

/// その target・水準の行が、上の設定でファイルに残るか（試験用。fern の判定をそのまま使う）。
#[cfg(test)]
pub(crate) fn keeps(target: &str, level: log::Level) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tauri_plugin_log::fern;

    let hit = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&hit);
    let (_, logger) = LEVELS
        .iter()
        .fold(
            fern::Dispatch::new().level(DEFAULT_LEVEL),
            |d, (target, level)| d.level_for(*target, *level),
        )
        .chain(fern::Output::call(move |_| {
            seen.store(true, Ordering::SeqCst)
        }))
        .into_log();
    logger.log(
        &log::Record::builder()
            .target(target)
            .level(level)
            .args(format_args!("試験"))
            .build(),
    );
    hit.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    #[test]
    fn imap_connection_lines_are_kept() {
        assert!(keeps("rondine_lib::services::imap_conn::gate", Level::Info));
        assert!(keeps("rondine_lib::services::imap_conn", Level::Warn));
        assert!(keeps("rondine_lib::commands", Level::Error));
    }

    #[test]
    fn noisy_dependency_details_are_dropped() {
        assert!(!keeps("reqwest::connect", Level::Trace));
        assert!(!keeps("hyper_util::client::legacy::pool", Level::Debug));
        assert!(!keeps("keyring", Level::Debug));
        assert!(!keeps(
            "tao::platform_impl::platform::event_loop",
            Level::Trace
        ));
        // 依存クレートでも警告は残す。
        assert!(keeps("reqwest::connect", Level::Warn));
    }
}
