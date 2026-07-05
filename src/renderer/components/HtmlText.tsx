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

/** テキスト中のメールアドレスを検出するための正規表現（キャプチャ付き）。 */
const EMAIL_RE = /([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g;

/** テキストノード内のメールアドレスを renderEmail で描画（クリックで新規作成／＋登録）。 */
function renderTextWithEmails(
  text: string,
  renderEmail: (email: string) => ReactNode,
): ReactNode {
  const parts = text.split(EMAIL_RE);
  return parts.map((p, i) =>
    i % 2 === 1 ? <Fragment key={i}>{renderEmail(p)}</Fragment> : p,
  );
}

function renderNode(
  node: Node,
  key: number,
  inlineImages: InlineImages,
  remoteImages: RemoteImages,
  remoteDefaultExpanded: boolean,
  renderEmail?: (email: string) => ReactNode,
): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent ?? '';
    if (!renderEmail || !text) return text;
    return renderTextWithEmails(text, renderEmail);
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
    children.push(renderNode(c, i, inlineImages, remoteImages, remoteDefaultExpanded, renderEmail)),
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

/** リンク（HTML 本文とプレーン本文で共通の見た目）。下線なしの水色・折返し可。 */
const LINK_CLASS = 'cursor-pointer text-sky-400 no-underline hover:text-sky-300 break-all';

/** プレーン本文中の URL（http(s)/ www. 始まり）とメールアドレスを 1 パスで検出する。 */
const AUTOLINK_RE =
  /((?:https?:\/\/|www\.)[^\s<>]+|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g;

/** URL 末尾に紛れがちな句読点・閉じ括弧をリンクから外す（表示テキストには残す）。 */
function stripTrailingPunct(url: string): [string, string] {
  const m = url.match(/[)\]}>.,;:!?'"、。）」』】]+$/);
  return m ? [url.slice(0, -m[0].length), m[0]] : [url, ''];
}

/**
 * プレーン本文を、URL・メールアドレスをリンク化して描画する。
 * 会話バブルと全文表示（プレーン経路）で共有し、リンクの見た目とクリック挙動
 *（外部ブラウザで開く）を HTML 本文（HtmlText）と揃えるためのコンポーネント。
 * - URL: 水色リンク。クリックで外部ブラウザ（親要素へは伝播させない＝バブルを開かない）。
 * - メール: renderEmail があればそれで描画（＋登録／新規作成の導線）、無ければ素のテキスト。
 */
export function AutoLinkText({
  text,
  renderEmail,
  className = '',
}: {
  text: string;
  renderEmail?: (email: string) => ReactNode;
  className?: string;
}) {
  const nodes: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const m of text.matchAll(AUTOLINK_RE)) {
    const match = m[0];
    const offset = m.index ?? 0;
    if (offset > last) nodes.push(text.slice(last, offset));
    last = offset + match.length;

    const isUrl = /^(https?:\/\/|www\.)/i.test(match);
    if (!isUrl && match.includes('@')) {
      // メールアドレス。導線があればそれで、無ければ素のテキスト（メールアプリなので mailto は張らない）。
      nodes.push(
        renderEmail ? <Fragment key={key++}>{renderEmail(match)}</Fragment> : match,
      );
      continue;
    }
    const [core, trail] = stripTrailingPunct(match);
    const href = core.startsWith('www.') ? `https://${core}` : core;
    nodes.push(
      <a
        key={key++}
        href={href}
        onClick={(e) => {
          e.preventDefault();
          // バブル内のクリックで全文展開が誘発されないよう伝播を止める。
          e.stopPropagation();
          openExternal(href);
        }}
        className={LINK_CLASS}
      >
        {core}
      </a>,
    );
    if (trail) nodes.push(trail);
  }
  if (last < text.length) nodes.push(text.slice(last));
  return <pre className={`whitespace-pre-wrap break-words font-sans ${className}`}>{nodes}</pre>;
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
  renderEmail,
}: {
  html: string;
  inlineImages?: InlineImages;
  remoteImages?: RemoteImages;
  /** リモート画像の初期サイズを完全表示にするか（既定はサムネイル。各画像はクリックで切替）。 */
  remoteDefaultExpanded?: boolean;
  /** 本文テキスト/ mailto 中のメールアドレスの描画（クリックで新規作成・＋登録）。 */
  renderEmail?: (email: string) => ReactNode;
}) {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return <>{html}</>;
  }
  const nodes: ReactNode[] = [];
  doc.body.childNodes.forEach((c, i) =>
    nodes.push(renderNode(c, i, inlineImages, remoteImages, remoteDefaultExpanded, renderEmail)),
  );
  return (
    <div className="break-words text-sm leading-relaxed text-white/90 [&_a]:break-all">{nodes}</div>
  );
}
