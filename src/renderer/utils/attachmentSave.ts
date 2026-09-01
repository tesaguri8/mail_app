import { open, save } from '@tauri-apps/plugin-dialog';
import { downloadDir, join } from '@tauri-apps/api/path';
import { attachmentExport } from '../services/mail';
import { withActivity } from '../stores/activity';

/**
 * 添付 1 件を「名前を付けて保存」ダイアログでユーザー指定の場所へ保存する（ダウンロード）。
 * 既定はダウンロードフォルダ。保存中はフッターに進捗（不確定スピナー）を出す。
 * 会話バブルと全文表示（MailBody）で共通利用する。
 *
 * @returns 実際に保存したら true、ダイアログをキャンセルしたら false。
 */
export async function saveAttachment(
  id: number,
  filename: string,
  activityLabel: string,
): Promise<boolean> {
  let defaultPath = filename;
  try {
    defaultPath = await join(await downloadDir(), filename);
  } catch {
    /* ダウンロードフォルダを解決できなければファイル名だけを既定にする */
  }
  const dest = await save({ defaultPath }).catch(() => null);
  if (!dest) return false;
  await withActivity(activityLabel, () => attachmentExport(id, dest));
  return true;
}

/**
 * 同じフォルダへ書き出すときに名前が衝突しないよう、2 件目以降へ連番を付ける。
 * `a.png` が 2 つあれば `a.png` / `a (2).png` になる。付けないと後の 1 件が
 * 前の 1 件を黙って上書きしてしまう（添付は元ファイル名が重複しやすい）。
 */
export function uniqueNames(filenames: string[]): string[] {
  const used = new Map<string, number>();
  return filenames.map((name) => {
    const key = name.toLowerCase();
    const seen = used.get(key) ?? 0;
    used.set(key, seen + 1);
    if (seen === 0) return name;
    const dot = name.lastIndexOf('.');
    // 先頭のドット（.gitignore 等）は拡張子ではないので末尾に付ける。
    return dot > 0
      ? `${name.slice(0, dot)} (${seen + 1})${name.slice(dot)}`
      : `${name} (${seen + 1})`;
  });
}

/**
 * 添付をまとめて、選んだフォルダへ保存する（ダウンロード）。
 * 1 件ずつ「名前を付けて保存」を出すのは煩雑なので、**フォルダを 1 回だけ**選ばせる。
 * 会話バブルのメニューと全文表示（MailBody）で共通利用する。
 *
 * 個別の失敗はスキップして残りを続ける（1 件壊れていても他が保存できるように）。
 *
 * @returns 保存できた件数。フォルダ選択をキャンセルしたら null。
 */
export async function saveAllAttachments(
  items: { id: number; filename: string }[],
  activityLabel: string,
): Promise<number | null> {
  let defaultPath: string | undefined;
  try {
    defaultPath = await downloadDir();
  } catch {
    /* 解決できなければダイアログの既定に任せる */
  }
  const picked = await open({ directory: true, defaultPath }).catch(() => null);
  const dir = typeof picked === 'string' ? picked : null;
  if (!dir) return null;

  const names = uniqueNames(items.map((a) => a.filename));
  let ok = 0;
  await withActivity(activityLabel, async () => {
    for (const [i, a] of items.entries()) {
      try {
        await attachmentExport(a.id, await join(dir, names[i]));
        ok += 1;
      } catch {
        /* 個別の失敗はスキップ */
      }
    }
  });
  return ok;
}
