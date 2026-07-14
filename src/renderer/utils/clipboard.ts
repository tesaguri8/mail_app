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
