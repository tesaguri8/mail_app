import { attachmentView } from '../services/mail';

/**
 * 添付画像の表示用 data URL のメモリキャッシュ（画面をまたいで共有）。
 *
 * 画像の表示は毎回 `attachment_view` を呼ぶと、ディスクに原本があっても Rust 側で
 * デコード→縮小→JPEG 化→base64 が走り、大きな写真ほど待たされる。バブルや本文は
 * メールを切り替えるたびにアンマウントされるので、コンポーネントの state だけでは
 * 「戻ったらまた最初から」になってしまう。ここで id 単位に持ち回して即表示にする。
 *
 * - 同じ画像への同時要求は 1 本にまとめる（in-flight 共有）。
 * - 合計サイズが上限を超えたら、参照の古いものから捨てる（素朴な LRU）。
 */

/** メモリに載せる data URL の合計上限（文字数 ≒ バイト数）。 */
const MAX_CHARS = 32 * 1024 * 1024;

/** key -> data URL。Map の挿入順を参照順として使う（get のたびに末尾へ入れ直す）。 */
const cache = new Map<string, string>();
/** 取得中の要求（同じ画像を同時に何度も取りに行かないため）。 */
const inflight = new Map<string, Promise<string>>();
let chars = 0;

function cacheKey(id: number, thumb: boolean): string {
  return `${id}:${thumb ? 't' : 'v'}`;
}

/** 上限を超えているぶんだけ、参照の古いものから捨てる。 */
function evict(): void {
  for (const [key, url] of cache) {
    if (chars <= MAX_CHARS) break;
    cache.delete(key);
    chars -= url.length;
  }
}

function put(key: string, url: string): void {
  cache.set(key, url);
  chars += url.length;
  evict();
}

/**
 * 取得済みなら data URL を同期で返す（未取得なら undefined）。
 * 「戻ってきたときに待たせない」ため、描画の初期値としてそのまま使える。
 */
export function peekAttachmentImage(id: number, thumb = false): string | undefined {
  return cache.get(cacheKey(id, thumb));
}

/**
 * 添付画像を表示用 data URL で取得する（取得済みならメモリから即返す）。
 * `thumb` は一覧サムネイル用の小さいレンディション。
 */
export function attachmentImage(id: number, thumb = false): Promise<string> {
  const key = cacheKey(id, thumb);
  const hit = cache.get(key);
  if (hit !== undefined) {
    // 参照したものを末尾へ（LRU の更新）。
    cache.delete(key);
    cache.set(key, hit);
    return Promise.resolve(hit);
  }
  const pending = inflight.get(key);
  if (pending) return pending;
  const req = attachmentView(id, thumb)
    .then((url) => {
      put(key, url);
      inflight.delete(key);
      return url;
    })
    .catch((e) => {
      inflight.delete(key);
      throw e;
    });
  inflight.set(key, req);
  return req;
}
