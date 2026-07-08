import { Fragment, useState, type ReactNode } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';
import { matchDates } from '../utils/dateparse';

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
  renderEmail: ((email: string) => ReactNode) | undefined,
  renderDate: ((raw: string) => ReactNode) | undefined,
  highlightRe: RegExp | null,
  insideLink = false,
): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent ?? '';
    if (!text) return text;
    // 既に <a> の内側のテキストは、リンクの二重化を避けて再リンク化しない（ハイライトのみ）。
    if (insideLink) return highlightText(text, highlightRe, `hl${key}`);
    // 生の URL / メールアドレス / 日付を自動リンク化（プレーンの AutoLinkText と同じロジック）。
    return linkifyToNodes(text, renderEmail, highlightRe, renderDate);
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

  // <a> の内側では子テキストを再リンク化しない（insideLink を子へ伝播）。
  const insideChildLink = insideLink || tag === 'a';
  const children: ReactNode[] = [];
  el.childNodes.forEach((c, i) =>
    children.push(
      renderNode(
        c,
        i,
        inlineImages,
        remoteImages,
        remoteDefaultExpanded,
        renderEmail,
        renderDate,
        highlightRe,
        insideChildLink,
      ),
    ),
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
          // バブル等でリンククリックが親の展開トグルへ伝播しないよう止める。
          e.stopPropagation();
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

/** 検索語（複数）を大文字小文字無視でマッチする正規表現を作る（無ければ null）。 */
export function buildHighlightRe(terms: string[] | undefined): RegExp | null {
  const esc = (terms ?? [])
    .map((t) => t.trim())
    .filter(Boolean)
    .map((t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  if (esc.length === 0) return null;
  // 長い語を先に（部分被りで短い方が先に食わないように）。
  esc.sort((a, b) => b.length - a.length);
  return new RegExp(`(${esc.join('|')})`, 'gi');
}

/** テキストを、検索語を <mark> で囲んで描画する。各マッチに data-search-match を付ける。 */
function highlightText(text: string, re: RegExp | null, keyBase: string): ReactNode {
  if (!re) return text;
  re.lastIndex = 0;
  const parts = text.split(re);
  if (parts.length === 1) return text;
  return parts.map((p, i) =>
    i % 2 === 1 ? (
      <mark key={`${keyBase}-${i}`} data-search-match className="search-hl">
        {p}
      </mark>
    ) : (
      <Fragment key={`${keyBase}-${i}`}>{p}</Fragment>
    ),
  );
}

/** プレーン本文中の URL（http(s)/ www. 始まり）とメールアドレスを 1 パスで検出する。 */
const AUTOLINK_RE =
  /((?:https?:\/\/|www\.)[^\s<>]+|[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g;

/** URL 末尾に紛れがちな句読点・閉じ括弧をリンクから外す（表示テキストには残す）。 */
function stripTrailingPunct(url: string): [string, string] {
  const m = url.match(/[)\]}>.,;:!?'"、。）」』】]+$/);
  return m ? [url.slice(0, -m[0].length), m[0]] : [url, ''];
}

/**
 * プレーンテキストを、URL・メールアドレスをリンク化した ReactNode 配列にする。
 * プレーン本文（AutoLinkText）と HTML 本文のテキストノード（HtmlText）で共有し、生の
 * URL/メールの見た目とクリック挙動（外部ブラウザで開く）を揃える。
 * - URL: 水色リンク。クリックで外部ブラウザ（親要素へは伝播させない＝バブルを開かない）。
 * - メール: renderEmail があればそれで描画（＋登録/新規作成の導線）、無ければ素のテキスト。
 * - 検索語（re）は一致部分を <mark> でハイライトする。
 */
function linkifyToNodes(
  text: string,
  renderEmail: ((email: string) => ReactNode) | undefined,
  re: RegExp | null,
  renderDate?: (raw: string) => ReactNode,
): ReactNode[] {
  const nodes: ReactNode[] = [];
  let last = 0;
  let key = 0;
  const pushText = (s: string) => {
    if (!s) return;
    // 日付導線があれば、URL/メール以外のプレーン部分をさらに日付で分割して＋を差し込む。
    if (renderDate) {
      const dates = matchDates(s);
      if (dates.length > 0) {
        let p = 0;
        for (const dm of dates) {
          if (dm.index > p)
            nodes.push(<Fragment key={key++}>{highlightText(s.slice(p, dm.index), re, `lt${key}`)}</Fragment>);
          nodes.push(<Fragment key={key++}>{renderDate(dm.raw)}</Fragment>);
          p = dm.index + dm.raw.length;
        }
        if (p < s.length)
          nodes.push(<Fragment key={key++}>{highlightText(s.slice(p), re, `lt${key}`)}</Fragment>);
        return;
      }
    }
    nodes.push(<Fragment key={key++}>{highlightText(s, re, `lt${key}`)}</Fragment>);
  };
  for (const m of text.matchAll(AUTOLINK_RE)) {
    const match = m[0];
    const offset = m.index ?? 0;
    if (offset > last) pushText(text.slice(last, offset));
    last = offset + match.length;

    const isUrl = /^(https?:\/\/|www\.)/i.test(match);
    if (!isUrl && match.includes('@')) {
      // メールアドレス。導線があればそれで、無ければ素のテキスト（メールアプリなので mailto は張らない）。
      if (renderEmail) nodes.push(<Fragment key={key++}>{renderEmail(match)}</Fragment>);
      else pushText(match);
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
          // クリックが親（バブルの展開トグル等）へ伝播しないよう止める。
          e.stopPropagation();
          openExternal(href);
        }}
        className={LINK_CLASS}
      >
        {core}
      </a>,
    );
    if (trail) pushText(trail);
  }
  if (last < text.length) pushText(text.slice(last));
  return nodes;
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
  renderDate,
  highlight,
  className = '',
}: {
  text: string;
  renderEmail?: (email: string) => ReactNode;
  /** 本文中の日付の描画（ホバーで＋・クリックでカレンダー入力）。 */
  renderDate?: (raw: string) => ReactNode;
  /** 検索語（複数）。本文中の一致を <mark> でハイライトする。 */
  highlight?: string[];
  className?: string;
}) {
  const re = buildHighlightRe(highlight);
  return (
    <pre className={`whitespace-pre-wrap break-words font-sans ${className}`}>
      {linkifyToNodes(text, renderEmail, re, renderDate)}
    </pre>
  );
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
  renderDate,
  highlight,
}: {
  html: string;
  inlineImages?: InlineImages;
  remoteImages?: RemoteImages;
  /** リモート画像の初期サイズを完全表示にするか（既定はサムネイル。各画像はクリックで切替）。 */
  remoteDefaultExpanded?: boolean;
  /** 本文テキスト/ mailto 中のメールアドレスの描画（クリックで新規作成・＋登録）。 */
  renderEmail?: (email: string) => ReactNode;
  /** 本文中の日付の描画（ホバーで＋・クリックでカレンダー入力）。 */
  renderDate?: (raw: string) => ReactNode;
  /** 検索語（複数）。本文中の一致を <mark> でハイライトする。 */
  highlight?: string[];
}) {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return <>{html}</>;
  }
  const re = buildHighlightRe(highlight);
  const nodes: ReactNode[] = [];
  doc.body.childNodes.forEach((c, i) =>
    nodes.push(
      renderNode(c, i, inlineImages, remoteImages, remoteDefaultExpanded, renderEmail, renderDate, re),
    ),
  );
  return (
    <div className="break-words text-sm leading-relaxed text-white/90 [&_a]:break-all">{nodes}</div>
  );
}
