// Windows リリースビルドではコンソールウィンドウを非表示
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    rondine_lib::run();
}
