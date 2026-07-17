import { save } from '@tauri-apps/plugin-dialog';
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
