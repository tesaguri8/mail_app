//! データ保存先（mail.db と attachments を置くフォルダ）の解決。
//!
//! 優先順位: `RONDINE_DATA_DIR` 環境変数 > ポインタファイル（ユーザーが選んだ移動先）> 既定。
//! 「どこに保存するか」自体はデータフォルダの外（既定 app_data 直下の小さなポインタファイル）に
//! 置く。デバッグビルドはワークツリーごとに別フォルダ／別ポインタにして、複数ブランチが同じ
//! DB を共有して migration 番号が衝突するのを防ぐ（release は共有の `data/`）。

use std::path::{Path, PathBuf};

#[cfg(debug_assertions)]
fn worktree_tag() -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("dev")
        .to_string()
}

/// このビルドの既定データフォルダ。
pub fn default_data_dir(base: &Path) -> PathBuf {
    #[cfg(debug_assertions)]
    {
        base.join(format!("data-dev-{}", worktree_tag()))
    }
    #[cfg(not(debug_assertions))]
    {
        base.join("data")
    }
}

/// 保存先を記録するポインタファイル（データの外＝先に読めるよう既定 base 直下に置く）。
pub fn pointer_file(base: &Path) -> PathBuf {
    #[cfg(debug_assertions)]
    {
        base.join(format!(".data-location-{}.txt", worktree_tag()))
    }
    #[cfg(not(debug_assertions))]
    {
        base.join(".data-location.txt")
    }
}

/// 実際に使うデータフォルダを解決する。
pub fn resolve_data_dir(base: &Path) -> PathBuf {
    if let Ok(d) = std::env::var("RONDINE_DATA_DIR") {
        let t = d.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    if let Ok(s) = std::fs::read_to_string(pointer_file(base)) {
        let t = s.trim();
        if !t.is_empty() {
            return PathBuf::from(t);
        }
    }
    default_data_dir(base)
}

/// mail.db のフルパス。
pub fn db_path(base: &Path) -> PathBuf {
    resolve_data_dir(base).join("mail.db")
}
