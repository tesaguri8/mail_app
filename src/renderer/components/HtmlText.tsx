import { Fragment, useState, type ReactNode } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** http(s)/mailto のみ許可。javascript: などは無効化する。 */
function safeHref(href: string | null): string | null {
  if (!href) return null;
  const v = href.trim();
  if (/^(https?:|mailto:)/i.test(v)) return v;
  return null;
}

/**
 * 画像 src がリモート取得可能な http(s)（プロトコル相対 //host も含む）なら
 * 絶対 URL へ正規化して返す。cid:/data: や相対パスは対象外（null）。
 */
function remoteSrc(src: string): string | null {
  const v = src.trim();
  if (/^https?:\/\//i.test(v)) return v;
  if (v.startsWith('//')) return `https:${v}`;
  return null;
}

function openExternal(url: string) {
  if (isTauri) openUrl(url).catch(() => undefined);
  else window.open(url, '_blank', 'noopener,noreferrer');
}

// テキストとして改行・段落を作るブロック要素
const BLOCK = new Set([
  'p',
  'div',
  'br',
  'tr',
  'li',
  'ul',
  'ol',
  'table',
  'h1',
  'h2',
  'h3',
  'h4',
  'h5',
  'h6',
  'blockquote',
]);
// 中身を捨てる要素（スクリプト・スタイル等）。img は cid: 解決時のみ描画する。
const DROP = new Set(['script', 'style', 'head', 'title', 'noscript', 'iframe', 'svg']);

/** 本文埋め込み画像（content_id → data URL）。リモート画像は対象外（ブロック）。 */
type InlineImages = Record<string, string>;
/** 許可して取得したリモート画像（正規化 URL → サニタイズ済み data URL）。 */
type RemoteImages = Record<string, string>;

/**
 * 許可済みリモート画像。既定はサムネイル（小さくインライン）で、クリックすると
 * 完全表示⇄サムネを切替える。defaultExpanded=true なら最初から完全表示で描画。
 */
function RemoteImg({
  src,
  alt,
  defaultExpanded,
}: {
  src: string;
  alt: string;
  defaultExpanded: boolean;
}) {
  const [expanded, setExpanded] = useState(defaultExpanded);
  return (
    <img
      src={src}
      alt={alt}
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        setExpanded((v) => !v);
      }}
      className={
        expanded
          ? 'my-2 block max-h-[480px] max-w-full cursor-zoom-out rounded-md'
          : 'my-1 mr-1 inline-block max-h-24 max-w-[180px] cursor-zoom-in rounded align-top'
      }
    />
  );
}

function renderNode(
  node: Node,
  key: number,
  inlineImages: InlineImages,
  remoteImages: RemoteImages,
  remoteDefaultExpanded: boolean,
): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? '';
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return null;

  const el = node as Element;
  const tag = el.tagName.toLowerCase();
  if (DROP.has(tag)) return null;

  // 画像: cid: 参照（解決済み）と、許可済みで取得できたリモート画像だけ表示。
  // それ以外のリモート(http)は既定ブロックでプレースホルダのみ（トラッキング防止）。
  if (tag === 'img') {
    const src = (el.getAttribute('src') ?? '').trim();
    const alt = el.getAttribute('alt') ?? '';
    if (src.toLowerCase().startsWith('cid:')) {
      const cid = src.slice(4).replace(/^<|>$/g, '');
      const url = inlineImages[cid];
      if (url) {
        return (
          <img
            key={key}
            src={url}
            alt={alt}
            className="my-2 block max-h-[480px] max-w-full rounded-md"
          />
        );
      }
    }
    // 許可して取得済みのリモート画像は、サニタイズ済み data URL で表示する。
    const remote = remoteSrc(src);
    if (remote) {
      const loaded = remoteImages[remote];
      if (loaded) {
        return (
          <RemoteImg key={key} src={loaded} alt={alt} defaultExpanded={remoteDefaultExpanded} />
        );
      }
    }
    // 未解決 / 未許可のリモート画像はプレースホルダのみ（トラッキング防止）。
    // 親リンク(<a>)へクリックを伝播させず、誤ってリンク先へ飛ばないようにする。
    return (
      <span
        key={key}
        className="text-white/30"
        title={src}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
        }}
      >
        🖼{alt ? ` ${alt}` : ''}
      </span>
    );
  }

  const children: ReactNode[] = [];
  el.childNodes.forEach((c, i) =>
    children.push(renderNode(c, i, inlineImages, remoteImages, remoteDefaultExpanded)),
  );

  if (tag === 'br') return <br key={key} />;

  if (tag === 'a') {
    const href = safeHref(el.getAttribute('href'));
    if (!href) return <Fragment key={key}>{children}</Fragment>;
    return (
      <a
        key={key}
        href={href}
        onClick={(e) => {
          e.preventDefault();
          openExternal(href);
        }}
        // リンクは下線なしの水色
        className="cursor-pointer text-sky-400 no-underline hover:text-sky-300"
      >
        {children}
      </a>
    );
  }

  if (BLOCK.has(tag)) {
    return <div key={key}>{children}</div>;
  }
  // それ以外はインラインとして中身だけ
  return <Fragment key={key}>{children}</Fragment>;
}

/**
 * HTML 本文に含まれるリモート画像（http(s)）の正規化 URL を重複なく集める。
 * 「外部画像 N 個・[画像を表示]」バナーの判定と一括取得に使う（docs/MAIL_SECURITY.md §1）。
 */
export function remoteImageUrls(html: string): string[] {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return [];
  }
  const urls = new Set<string>();
  doc.querySelectorAll('img').forEach((img) => {
    const u = remoteSrc(img.getAttribute('src') ?? '');
    if (u) urls.add(u);
  });
  return [...urls];
}

/**
 * メールの HTML 本文を「テキスト＋リンク＋埋め込み画像」で安全に描画する。
 * - innerHTML は使わず DOM を走査して React 要素に変換（スクリプト実行なし）
 * - スクリプト/スタイルは描画しない。リモート(http)画像は既定ブロック（トラッキング防止）
 * - cid: 埋め込み画像は解決済み（inlineImages）のものだけ表示
 * - リモート画像は許可して取得したもの（remoteImages）だけをサニタイズ済み data URL で表示
 * - リンクは下線なしの水色。クリックは外部ブラウザで開く
 */
export function HtmlText({
  html,
  inlineImages = {},
  remoteImages = {},
  remoteDefaultExpanded = false,
}: {
  html: string;
  inlineImages?: InlineImages;
  remoteImages?: RemoteImages;
  /** リモート画像の初期サイズを完全表示にするか（既定はサムネイル。各画像はクリックで切替）。 */
  remoteDefaultExpanded?: boolean;
}) {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return <>{html}</>;
  }
  const nodes: ReactNode[] = [];
  doc.body.childNodes.forEach((c, i) =>
    nodes.push(renderNode(c, i, inlineImages, remoteImages, remoteDefaultExpanded)),
  );
  return (
    <div className="break-words text-sm leading-relaxed text-white/90 [&_a]:break-all">{nodes}</div>
  );
}
