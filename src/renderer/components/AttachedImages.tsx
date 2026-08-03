import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Image as ImageIcon } from 'lucide-react';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import { attachmentView } from '../services/mail';
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

/**
 * 本文の下に画像を並べて表示する（Thunderbird 等と同じく「写真だけのメール」をそのまま読める
 * ようにするため）。対象は呼び出し側が絞った画像パート（本文から cid: で参照されていない
 * 埋め込み画像など）。
 *
 * - 合計が [`AUTO_MAX_TOTAL`] 以下なら開いた時点で自動取得、超えるときはボタンで明示取得。
 * - 右クリックで「保存 / 既定アプリで開く」（`onMenu`）。
 */
export function AttachedImages({
  images,
  onMenu,
}: {
  images: AttachmentSummary[];
  /** 画像の右クリック（保存/開くメニューを出す）。 */
  onMenu?: (att: AttachmentSummary, x: number, y: number) => void;
}) {
  const { t } = useTranslation();
  const [urls, setUrls] = useState<Record<number, string>>({});
  const total = images.reduce((s, a) => s + a.size, 0);
  const [wanted, setWanted] = useState(total <= AUTO_MAX_TOTAL);
  const [busy, setBusy] = useState(false);
  // 対象が変わったら取得済みを捨てて、上限判定からやり直す（メール切替）。
  const key = images.map((a) => a.id).join(',');

  useEffect(() => {
    setUrls({});
    setWanted(total <= AUTO_MAX_TOTAL);
    // key（表示対象）が変わったときだけリセットする。total は key に従属。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  useEffect(() => {
    if (!wanted || images.length === 0) return;
    let alive = true;
    setBusy(true);
    // 本体未取得なら Rust 側が IMAP から該当パートだけ取るので、フッターに進捗を出す。
    void withActivity(t('activity.loadingImages'), async () => {
      for (const a of images) {
        if (!alive) return;
        try {
          const url = await attachmentView(a.id);
          if (!alive) return;
          setUrls((prev) => ({ ...prev, [a.id]: url }));
        } catch {
          /* この 1 枚は表示できない（他は続ける） */
        }
      }
    }).finally(() => {
      if (alive) setBusy(false);
    });
    return () => {
      alive = false;
    };
    // images は毎レンダー新しい配列になるので、対象の同一性は key で見る。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [wanted, key, t]);

  if (images.length === 0) return null;

  if (!wanted) {
    return (
      <button
        onClick={() => setWanted(true)}
        className="mt-3 flex items-center gap-1.5 rounded-md border border-white/15 px-2.5 py-1.5 text-xs text-white/70 hover:bg-white/10 hover:text-white"
      >
        <ImageIcon size={13} />
        {t('mailbox.showImages', { count: images.length })}・{formatSize(total)}
      </button>
    );
  }

  return (
    <div className="mt-3 space-y-3">
      {images.map((a) => (
        <figure key={a.id} className="min-w-0">
          <figcaption className="mb-1 truncate text-[11px] text-white/40">
            {a.filename}・{formatSize(a.size)}
          </figcaption>
          {urls[a.id] ? (
            <img
              src={urls[a.id]}
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
            <div className="rounded-md border border-white/10 px-3 py-4 text-center text-[11px] text-white/35">
              {busy ? t('mailbox.attachmentBusy') : t('mailbox.imageUnavailable')}
            </div>
          )}
        </figure>
      ))}
    </div>
  );
}
