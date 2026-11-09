// 作成画面の別ウィンドウ（docs/COMPOSE.md §別ウィンドウ）。
// 書きかけの内容は Rust に預け、新しいウィンドウが自分の label で受け取る（URL に載せない）。
import { invoke } from '@tauri-apps/api/core';
import type { ComposeHandoff } from '@bindings/ComposeHandoff';

/** 作成ウィンドウの label の接頭辞（src-tauri の compose_window::LABEL_PREFIX と揃える）。 */
const LABEL_PREFIX = 'compose-';

/**
 * 作成ウィンドウが下書きを保存・送信・破棄して閉じたことを、メインのウィンドウへ知らせる
 * Tauri イベント（メインは一覧を読み直す。送った分は Rust の "sync:sent" で別に届く）。
 */
export const COMPOSE_WINDOW_DONE_EVENT = 'compose:done';

/** 書きかけの内容を預けて、作成ウィンドウを開く。開いたウィンドウの label を返す。 */
export const composeWindowOpen = (handoff: ComposeHandoff) =>
  invoke<string>('compose_window_open', { handoff });

/** このウィンドウに預けてある書きかけの内容（無ければ null）。 */
export const composeWindowHandoff = () => invoke<ComposeHandoff | null>('compose_window_handoff');

/** このウィンドウの label が作成ウィンドウのものか。 */
export function isComposeWindowLabel(label: string): boolean {
  return label.startsWith(LABEL_PREFIX);
}

/** このウィンドウの預かりを今の内容で置き換える（下書きを保存するたびに呼ぶ）。 */
export const composeWindowSaveState = (handoff: ComposeHandoff) =>
  invoke<void>('compose_window_save_state', { handoff });
