use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 境界型の例。ts-rs により `src/bindings/AppInfo.ts` を生成する。
/// 生成: `npm run gen:bindings`（= cargo test --lib export_bindings）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
    pub identifier: String,
}

/// データベースの状態（スキーマバージョン・パス）。
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct DbInfo {
    pub schema_version: i32,
    pub path: String,
}
