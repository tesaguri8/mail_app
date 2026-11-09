//! 作成画面の別ウィンドウ（docs/COMPOSE.md §別ウィンドウ）。
//!
//! メインの作成画面が書きかけの内容（[`ComposeHandoff`]）を預けると、`compose-<連番>` の
//! label で新しいウィンドウを開く。新しいウィンドウは自分の label で預けた内容を受け取る。
//! 内容は URL に載せず、ここで label ごとに持つ（本文や添付の一覧は大きくなりうるため）。
//! 預けた内容はウィンドウが閉じるまで残す（開発時の再読み込みや StrictMode の二重実行でも
//! 同じ内容を受け取れるように）。

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::models::ComposeHandoff;

/// 作成ウィンドウの label の接頭辞（capabilities/compose.json の `compose-*` と揃える）。
pub const LABEL_PREFIX: &str = "compose-";

/// 元メールを右に並べないときの大きさ（論理ピクセル）。
const SIZE_SINGLE: (f64, f64) = (760.0, 760.0);
/// 返信・転送など、元メールを右に並べるときの大きさ。
const SIZE_WITH_SOURCE: (f64, f64) = (1200.0, 800.0);
/// 最小の大きさ（メインのウィンドウと同じ下限より少し小さく。作成欄が潰れない幅）。
const MIN_SIZE: (f64, f64) = (480.0, 480.0);

/// 作成ウィンドウの操作の失敗。
#[derive(Debug, thiserror::Error)]
pub enum ComposeWindowError {
    #[error("作成ウィンドウを開けませんでした: {0}")]
    Build(#[from] tauri::Error),
    #[error("作成ウィンドウの預かりの状態が壊れています")]
    Poisoned,
}

/// 作成ウィンドウに渡す内容の預かり場所（`app.manage` で 1 つ持つ）。
#[derive(Default)]
pub struct ComposeWindows {
    next: AtomicU32,
    handoffs: Mutex<HashMap<String, ComposeHandoff>>,
}

impl ComposeWindows {
    /// 次の label を払い出し、内容を預ける。
    pub fn reserve(&self, handoff: ComposeHandoff) -> Result<String, ComposeWindowError> {
        let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let label = format!("{LABEL_PREFIX}{n}");
        self.handoffs
            .lock()
            .map_err(|_| ComposeWindowError::Poisoned)?
            .insert(label.clone(), handoff);
        Ok(label)
    }

    /// その label に預けた内容（無ければ None）。取り出さずに写しを返す。
    pub fn get(&self, label: &str) -> Result<Option<ComposeHandoff>, ComposeWindowError> {
        Ok(self
            .handoffs
            .lock()
            .map_err(|_| ComposeWindowError::Poisoned)?
            .get(label)
            .cloned())
    }

    /// 預けた内容を、そのウィンドウの今の内容で置き換える（下書きを保存するたびに呼ぶ）。
    /// 再読み込みしても、最後に保存した内容と下書きの id から続けられるように。
    /// 預かりの無い label（作成ウィンドウ以外・閉じた後）には何もしない。
    pub fn update(&self, label: &str, handoff: ComposeHandoff) -> Result<(), ComposeWindowError> {
        let mut map = self
            .handoffs
            .lock()
            .map_err(|_| ComposeWindowError::Poisoned)?;
        if let Some(slot) = map.get_mut(label) {
            *slot = handoff;
        }
        Ok(())
    }

    /// 預けた内容を捨てる（ウィンドウが閉じたとき・開けなかったとき）。
    pub fn release(&self, label: &str) {
        if let Ok(mut map) = self.handoffs.lock() {
            map.remove(label);
        }
    }
}

/// 作成ウィンドウの label か。
pub fn is_compose_label(label: &str) -> bool {
    label.starts_with(LABEL_PREFIX)
}

/// 内容を預けて、新しい作成ウィンドウを開く。開いたウィンドウの label を返す。
///
/// 見た目はメインと同じく枠なし（自前のタイトル部で移動・最小化・最大化・閉じる）。
/// ファイルのドロップは HTML 側で受けるので、ウィンドウのドロップ処理は切る（メインと同じ）。
///
/// # Errors
/// ウィンドウを作れなかったとき（預けた内容は捨てる）。
pub fn open(app: &AppHandle, handoff: ComposeHandoff) -> Result<String, ComposeWindowError> {
    let windows = app.state::<ComposeWindows>();
    let (w, h) = if handoff.source_id.is_some() {
        SIZE_WITH_SOURCE
    } else {
        SIZE_SINGLE
    };
    let title = app.config().product_name.clone().unwrap_or_default();
    let label = windows.reserve(handoff)?;
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::default())
        .title(title)
        .inner_size(w, h)
        .min_inner_size(MIN_SIZE.0, MIN_SIZE.1)
        .decorations(false)
        .resizable(true)
        .center()
        .disable_drag_drop_handler()
        .build();
    match built {
        Ok(_) => Ok(label),
        Err(e) => {
            windows.release(&label);
            Err(e.into())
        }
    }
}

/// 呼んだウィンドウに預けてある内容（作成ウィンドウ以外・預かりが無ければ None）。
///
/// # Errors
/// 預かりの状態が壊れているとき。
pub fn handoff_for(window: &WebviewWindow) -> Result<Option<ComposeHandoff>, ComposeWindowError> {
    window.state::<ComposeWindows>().get(window.label())
}

/// 呼んだ作成ウィンドウの預かりを、今の内容で置き換える。
///
/// # Errors
/// 預かりの状態が壊れているとき。
pub fn save_state(
    window: &WebviewWindow,
    handoff: ComposeHandoff,
) -> Result<(), ComposeWindowError> {
    window
        .state::<ComposeWindows>()
        .update(window.label(), handoff)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ComposeMode;

    fn handoff(subject: &str) -> ComposeHandoff {
        ComposeHandoff {
            mode: ComposeMode::New,
            source_id: None,
            draft_id: None,
            account_id: Some(1),
            to: "a@example.invalid, ".into(),
            cc: String::new(),
            bcc: String::new(),
            subject: subject.into(),
            body: "本文".into(),
            attachments: Vec::new(),
            signature_id: None,
            signature_block: String::new(),
            dirty: true,
            unsaved: true,
        }
    }

    #[test]
    fn labels_are_numbered_and_keep_their_own_handoff() {
        let w = ComposeWindows::default();
        let a = w.reserve(handoff("一通目")).expect("reserve");
        let b = w.reserve(handoff("二通目")).expect("reserve");
        assert_eq!(a, "compose-1");
        assert_eq!(b, "compose-2");
        assert!(is_compose_label(&a));
        assert!(!is_compose_label("main"));
        let got = w.get(&b).expect("get").map(|h| h.subject);
        assert_eq!(got.as_deref(), Some("二通目"));
    }

    #[test]
    fn handoff_survives_repeated_reads_until_released() {
        let w = ComposeWindows::default();
        let a = w.reserve(handoff("件名")).expect("reserve");
        // 再読み込み・StrictMode の二重実行で 2 回読んでも同じ内容が返る。
        assert!(w.get(&a).expect("get").is_some());
        assert!(w.get(&a).expect("get").is_some());
        w.release(&a);
        assert!(w.get(&a).expect("get").is_none());
        // 解放しても番号は使い回さない（閉じた窓の label と取り違えない）。
        assert_eq!(w.reserve(handoff("次")).expect("reserve"), "compose-2");
    }

    #[test]
    fn update_replaces_only_an_existing_handoff() {
        let w = ComposeWindows::default();
        let a = w.reserve(handoff("最初")).expect("reserve");
        let mut saved = handoff("保存後");
        saved.draft_id = Some(55);
        w.update(&a, saved).expect("update");
        let got = w.get(&a).expect("get").expect("handoff");
        assert_eq!((got.subject.as_str(), got.draft_id), ("保存後", Some(55)));
        // 閉じた後（解放済み）や、作成ウィンドウ以外の label には作らない。
        w.release(&a);
        w.update(&a, handoff("遅れて届いた")).expect("update");
        w.update("main", handoff("メイン")).expect("update");
        assert!(w.get(&a).expect("get").is_none());
        assert!(w.get("main").expect("get").is_none());
    }

    #[test]
    fn unknown_label_has_no_handoff() {
        let w = ComposeWindows::default();
        assert!(w.get("main").expect("get").is_none());
    }
}
