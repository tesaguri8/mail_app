import { open, save } from '@tauri-apps/plugin-dialog';
import { downloadDir, join } from '@tauri-apps/api/path';
import type { TFunction } from 'i18next';
import type { EmlAttachment } from '@bindings/EmlAttachment';
import {
  attachmentEmlExport,
  attachmentEmlOpen,
  attachmentExport,
  attachmentOpen,
} from '../services/mail';
import { withActivity } from '../stores/activity';
import { trackAttachment, type AttachmentRef } from '../stores/attachmentDownloads';

/** IMAP が上限の時間内に応答しなかったときに Rust 側が返す符号（commands.rs の ERR_IMAP_TIMEOUT）。 */
export const ERR_IMAP_TIMEOUT = 'imap.timeout';

/**
 * 添付の取得・保存・起動の失敗を、利用者に見せる理由の文に直す。
 * 時間切れは決まった符号で返ってくるので、分かる文言に置き換える。
 */
export function attachmentErrorReason(e: unknown, t: TFunction): string {
  const raw = e instanceof Error ? e.message : String(e);
  return raw === ERR_IMAP_TIMEOUT ? t('mailbox.attachmentTimeout') : raw;
}

/** まとめて保存した結果。 */
export interface SaveAllResult {
  /** 保存できた件数。 */
  saved: number;
  /** 保存できなかった件数。 */
  failed: number;
  /** 最初の失敗（failed が 0 なら undefined）。 */
  error?: unknown;
}

/**
 * まとめて保存した結果を、トーストに出す文と種類に直す（全部保存できたら成功、1 件でも失敗したら失敗）。
 */
export function saveAllMessage(
  r: SaveAllResult,
  t: TFunction
): { text: string; kind: 'ok' | 'error' } {
  if (r.failed === 0) {
    return { text: t('mailbox.attachmentSavedN', { count: r.saved }), kind: 'ok' };
  }
  return {
    text: t('mailbox.attachmentSavedPartial', {
      saved: r.saved,
      failed: r.failed,
      reason: attachmentErrorReason(r.error, t),
    }),
    kind: 'error',
  };
}

/**
 * 保存先のパスに使えない文字を `_` に置き換える（Rust 側の添付キャッシュと同じ方針）。
 * メールのファイル名には `/` や `:` が入ることがあり、そのままフォルダへ連結すると
 * 別の場所を指したり保存に失敗する。末尾のドット・空白は Windows が受け付けないので落とす
 * （先頭のドットは `.gitignore` のような正当な名前なので残す）。空になる名前は `attachment`。
 */
export function safeFilename(name: string): string {
  const cleaned = Array.from(name)
    .map((ch) => (/[/\\:*?"<>|]/.test(ch) || ch.codePointAt(0)! < 0x20 ? '_' : ch))
    .join('');
  const trimmed = cleaned.trim().replace(/[.\s]+$/, '');
  return trimmed === '' ? 'attachment' : trimmed;
}

/**
 * 添付 1 件を「名前を付けて保存」ダイアログでユーザー指定の場所へ保存する（ダウンロード）。
 * 既定はダウンロードフォルダ。取得中は添付の行に進み具合を、フッターにも作業を出す。
 * 会話バブルと全文表示（MailBody）で共通利用する。
 *
 * @returns 実際に保存したら true、ダイアログをキャンセルしたら false。
 */
export async function saveAttachment(a: AttachmentRef, activityLabel: string): Promise<boolean> {
  const safe = safeFilename(a.filename);
  let defaultPath = safe;
  try {
    defaultPath = await join(await downloadDir(), safe);
  } catch {
    /* ダウンロードフォルダを解決できなければファイル名だけを既定にする */
  }
  const dest = await save({ defaultPath }).catch(() => null);
  if (!dest) return false;
  await trackAttachment(a, () => withActivity(activityLabel, () => attachmentExport(a.id, dest)));
  return true;
}

/** 添付を OS の関連アプリで開く（未取得なら取得してから）。取得中は行とフッターに出す。 */
export async function openAttachment(a: AttachmentRef, activityLabel: string): Promise<void> {
  await trackAttachment(a, () => withActivity(activityLabel, () => attachmentOpen(a.id)));
}

/**
 * 添付された転送メール（.eml・`eml`）の中の添付 1 件を「名前を付けて保存」で保存する。
 * .eml が未取得なら先に取るので、その進み具合は .eml の行に出す。
 *
 * @returns 実際に保存したら true、ダイアログをキャンセルしたら false。
 */
export async function saveEmlPart(
  eml: AttachmentRef,
  part: EmlAttachment,
  activityLabel: string
): Promise<boolean> {
  const safe = safeFilename(part.filename);
  let defaultPath = safe;
  try {
    defaultPath = await join(await downloadDir(), safe);
  } catch {
    /* ダウンロードフォルダを解決できなければファイル名だけを既定にする */
  }
  const dest = await save({ defaultPath }).catch(() => null);
  if (!dest) return false;
  await trackAttachment(eml, () =>
    withActivity(activityLabel, () => attachmentEmlExport(eml.id, part.index, dest))
  );
  return true;
}

/** 添付された転送メール（.eml）の中の添付を OS の関連アプリで開く。 */
export async function openEmlPart(
  eml: AttachmentRef,
  index: number,
  activityLabel: string
): Promise<void> {
  await trackAttachment(eml, () =>
    withActivity(activityLabel, () => attachmentEmlOpen(eml.id, index))
  );
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
 * 失敗の件数と最初の失敗は結果に入れて返す（黙って捨てない）。
 *
 * @returns 保存できた件数・失敗の件数。フォルダ選択をキャンセルしたら null。
 */
export async function saveAllAttachments(
  items: AttachmentRef[],
  activityLabel: string
): Promise<SaveAllResult | null> {
  let defaultPath: string | undefined;
  try {
    defaultPath = await downloadDir();
  } catch {
    /* 解決できなければダイアログの既定に任せる */
  }
  const picked = await open({ directory: true, defaultPath }).catch(() => null);
  const dir = typeof picked === 'string' ? picked : null;
  if (!dir) return null;

  const names = uniqueNames(items.map((a) => safeFilename(a.filename)));
  const result: SaveAllResult = { saved: 0, failed: 0 };
  await withActivity(activityLabel, async () => {
    for (const [i, a] of items.entries()) {
      try {
        const dest = await join(dir, names[i]);
        await trackAttachment(a, () => attachmentExport(a.id, dest));
        result.saved += 1;
      } catch (e) {
        // 個別の失敗は数えて残りを続ける。
        result.failed += 1;
        result.error ??= e;
      }
    }
  });
  return result;
}
