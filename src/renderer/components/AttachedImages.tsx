import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Image as ImageIcon } from 'lucide-react';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import { attachmentImage, peekAttachmentImage } from '../utils/imageCache';
import { withActivity } from '../stores/activity';

const IMAGE_EXT = /\.(jpe?g|png|gif|webp|bmp|tiff?|heic|heif|avif)$/i;

/** 画像（変換すれば表示できる HEIC 等を含む）かどうか。 */
export function isImage(a: AttachmentSummary): boolean {
  if (a.content_type?.toLowerCase().startsWith('image/')) return true;
  return IMAGE_EXT.test(a.filename);
}

/**
 * 自動で読み込む合計サイズの上限。これを超えるときは自動取得せず、ボタンで明示的に読み込む
 * （本体未取得の添付はサーバーから取り直すため、開いただけで大量に通信しないようにする）。
 */
const AUTO_MAX_TOTAL = 25 * 1024 * 1024;

/** バイト数を人が読みやすい単位に整形する。 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** キャッシュ/state のキー（同じ画像でもサムネと実寸は別物として持つ）。 */
function variantKey(id: number, thumb: boolean): string {
  return `${id}:${thumb ? 't' : 'v'}`;
}

/**
 * 本文の下に画像を並べて表示する（Thunderbird 等と同じく「写真だけのメール」をそのまま読める
 * ようにするため）。対象は呼び出し側が絞った画像パート（本文から cid: で参照されていない
 * 埋め込み画像など）。
 *
 * - `compact`（会話バブル用）はサムネイルを横に並べ、クリックした 1 枚だけ実寸に広げる。
 *   チャットの流れを写真で埋めないための表示で、取得も小さいレンディションで済む。
 * - 合計が [`AUTO_MAX_TOTAL`] 以下なら開いた時点で自動取得、超えるときはボタンで明示取得。
 * - 右クリックで「保存 / 既定アプリで開く」（`onMenu`）。
 */
export function AttachedImages({
  images,
  onMenu,
  compact = false,
}: {
  images: AttachmentSummary[];
  /** 画像の右クリック（保存/開くメニューを出す）。 */
  onMenu?: (att: AttachmentSummary, x: number, y: number) => void;
  /** 会話バブル向けの小さい表示（サムネイル＋クリックで拡大）。 */
  compact?: boolean;
}) {
  const { t } = useTranslation();
  const total = images.reduce((s, a) => s + a.size, 0);
  // 対象の同一性はメール切替の検知に使う（images は毎レンダー新しい配列になるため）。
  const key = images.map((a) => a.id).join(',');
  // compact のときにクリックで実寸へ広げた画像。
  const [zoomed, setZoomed] = useState<Set<number>>(new Set());
  // その画像に必要なレンディション（true=サムネ）。
  const thumbFor = (id: number) => compact && !zoomed.has(id);
  // 読み込み済み（メモリキャッシュ）のぶんは最初の描画から出す＝別のメールから戻っても
  // 取り直さない・「処理中」表示も出さない。
  const cached = useMemo(() => {
    const seed: Record<string, string> = {};
    for (const a of images) {
      for (const thumb of [true, false]) {
        const url = peekAttachmentImage(a.id, thumb);
        if (url) seed[variantKey(a.id, thumb)] = url;
      }
    }
    return seed;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);
  const [urls, setUrls] = useState<Record<string, string>>(cached);
  const [wanted, setWanted] = useState(total <= AUTO_MAX_TOTAL);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setUrls(cached);
    setZoomed(new Set());
    setWanted(total <= AUTO_MAX_TOTAL);
    // key（表示対象）が変わったときだけやり直す。cached / total は key に従属。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  // 表示に必要なレンディションのうち、まだ手元に無いものを取りに行く。
  // 拡大（zoomed）で実寸が要るようになったときも、この効果が続きを取る。
  const needed = images.map((a) => ({ att: a, thumb: thumbFor(a.id) }));
  const missingKeys = needed
    .filter(({ att, thumb }) => peekAttachmentImage(att.id, thumb) === undefined)
    .map(({ att, thumb }) => variantKey(att.id, thumb))
    .join(',');

  useEffect(() => {
    if (!wanted || missingKeys === '') return;
    const missing = needed.filter(
      ({ att, thumb }) => peekAttachmentImage(att.id, thumb) === undefined,
    );
    if (missing.length === 0) return;
    let alive = true;
    setBusy(true);
    // 本体未取得なら Rust 側が IMAP から該当パートだけ取るので、フッターに進捗を出す。
    // 1 枚ずつ順に取る（同時に IMAP 接続を張らない）。
    const load = async () => {
      for (const { att, thumb } of missing) {
        if (!alive) return;
        try {
          const url = await attachmentImage(att.id, thumb);
          if (!alive) return;
          setUrls((prev) => ({ ...prev, [variantKey(att.id, thumb)]: url }));
        } catch {
          /* この 1 枚は表示できない（他は続ける） */
        }
      }
    };
    void withActivity(t('activity.loadingImages'), load).finally(() => {
      if (alive) setBusy(false);
    });
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted, missingKeys, t]);

  if (images.length === 0) return null;

  if (!wanted) {
    return (
      <button
        onClick={(e) => {
          // バブル内では親のクリック（展開トグル）へ伝播させない。
          e.stopPropagation();
          setWanted(true);
        }}
        className="mt-3 flex items-center gap-1.5 rounded-md border border-white/15 px-2.5 py-1.5 text-xs text-white/70 hover:bg-white/10 hover:text-white"
      >
        <ImageIcon size={13} />
        {t('mailbox.showImages', { count: images.length })}・{formatSize(total)}
      </button>
    );
  }

  const placeholder = (small: boolean) => (
    <div
      className={`flex items-center justify-center rounded-md border border-white/10 text-[11px] text-white/35 ${
        small ? 'h-24 w-24' : 'px-3 py-4'
      }`}
    >
      {busy ? t('mailbox.attachmentBusy') : t('mailbox.imageUnavailable')}
    </div>
  );

  // バブル: サムネイルを横に並べ、クリックした 1 枚だけ実寸に広げる。
  if (compact) {
    return (
      <div className="mt-2 flex flex-wrap gap-1.5">
        {images.map((a) => {
          const big = zoomed.has(a.id);
          const url = urls[variantKey(a.id, !big)];
          if (!url) return <div key={a.id}>{placeholder(!big)}</div>;
          return (
            <img
              key={a.id}
              src={url}
              alt={a.filename}
              title={`${a.filename}・${formatSize(a.size)}`}
              onClick={(e) => {
                // 画像のクリックはバブルの展開トグルへ渡さない（拡大/縮小だけ）。
                e.stopPropagation();
                setZoomed((prev) => {
                  const next = new Set(prev);
                  if (next.has(a.id)) next.delete(a.id);
                  else next.add(a.id);
                  return next;
                });
              }}
              onContextMenu={
                onMenu
                  ? (e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      onMenu(a, e.clientX, e.clientY);
                    }
                  : undefined
              }
              className={
                big
                  ? 'block max-h-[60vh] max-w-full cursor-zoom-out rounded-md'
                  : 'h-24 w-24 cursor-zoom-in rounded-md object-cover'
              }
            />
          );
        })}
      </div>
    );
  }

  return (
    <div className="mt-3 space-y-3">
      {images.map((a) => (
        <figure key={a.id} className="min-w-0">
          <figcaption className="mb-1 truncate text-[11px] text-white/40">
            {a.filename}・{formatSize(a.size)}
          </figcaption>
          {urls[variantKey(a.id, false)] ? (
            <img
              src={urls[variantKey(a.id, false)]}
              alt={a.filename}
              onContextMenu={
                onMenu
                  ? (e) => {
                      e.preventDefault();
                      e.stopPropagation();
                      onMenu(a, e.clientX, e.clientY);
                    }
                  : undefined
              }
              className="block max-h-[70vh] max-w-full rounded-md"
            />
          ) : (
            placeholder(false)
          )}
        </figure>
      ))}
    </div>
  );
}
