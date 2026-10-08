import { readText as tauriReadText } from '@tauri-apps/plugin-clipboard-manager';

/**
 * テキストをクリップボードへコピーする。navigator.clipboard を優先し、使えない環境では
 * 一時 textarea + execCommand にフォールバックする（一部の WebView では clipboard API が不可）。
 */
export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {
    // clipboard API が使えないときは下のフォールバックへ。
  }
  const ta = document.createElement('textarea');
  ta.value = text;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  try {
    document.execCommand('copy');
  } finally {
    document.body.removeChild(ta);
  }
}

/**
 * クリップボードのテキストを読む。読めないときは null を返す。
 *
 * WebKitGTK（Linux の WebView）では navigator.clipboard.readText が使えないため、
 * Tauri のクリップボードプラグインで読む（execCommand('paste') は現行の WebView で通らない）。
 */
export async function readText(): Promise<string | null> {
  try {
    const text = await tauriReadText();
    return text.length > 0 ? text : null;
  } catch {
    return null;
  }
}
