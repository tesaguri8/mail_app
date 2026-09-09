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

/**
 * 引用の段ごとの縦線の色。深さで色を変え、4 段目以降は循環させる（同色を重ねると
 * 深さが読めず、折りたたまれた引用を途中から読んだときに迷子になるため）。
 * 背景写真の上に載るので彩度は抑え、アプリの既存アクセント（sky / emerald / amber）に揃える。
 */
const QUOTE_BAR_COLORS = ['border-sky-400/50', 'border-emerald-400/50', 'border-amber-400/50'];

/** 引用ブロック（縦線＋字下げ）。HTML の blockquote とプレーンの「>」で同じ見た目を使う。 */
function QuoteBlock({ depth, children }: { depth: number; children: ReactNode }) {
  const color = QUOTE_BAR_COLORS[depth % QUOTE_BAR_COLORS.length];
  return (
    <blockquote className={`my-1 border-l-2 pl-2.5 text-white/70 ${color}`}>{children}</blockquote>
  );
}

/** 本文埋め込み画像（content_id → data URL）。リモート画像は対象外（ブロック）。 */
type InlineImages = Record<string, string>;
/** 許可して取得したリモート画像（正規化 URL → サニタイズ済み data URL）。 */
type RemoteImages = Record<string, string>;

/**
 * 描画中ずっと変わらない設定・コールバック一式。DOM を再帰的にたどる renderNode へ
 * 引数を一つずつ引き回さないようまとめて渡す。
 */
type RenderCtx = {
  inlineImages: InlineImages;
  remoteImages: RemoteImages;
  remoteDefaultExpanded: boolean;
  renderEmail?: (email: string) => ReactNode;
  renderDate?: (raw: string) => ReactNode;
  highlightRe: RegExp | null;
  /** 本文中のインライン画像を右クリックしたとき（保存/開く メニュー用。cid を渡す）。 */
  onInlineImageMenu?: (cid: string, x: number, y: number) => void;
};

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
  ctx: RenderCtx,
  insideLink = false,
  quoteDepth = 0,
): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    const text = node.textContent ?? '';
    if (!text) return text;
    // 既に <a> の内側のテキストは、リンクの二重化を避けて再リンク化しない（ハイライトのみ）。
    if (insideLink) return highlightText(text, ctx.highlightRe, `hl${key}`);
    // 生の URL / メールアドレス / 日付を自動リンク化（プレーンの AutoLinkText と同じロジック）。
    return linkifyToNodes(text, ctx.renderEmail, ctx.highlightRe, ctx.renderDate);
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
      const url = ctx.inlineImages[cid];
      if (url) {
        const onMenu = ctx.onInlineImageMenu;
        return (
          <img
            key={key}
            src={url}
            alt={alt}
            // 右クリックで「保存 / 開く」を出す（インライン画像は添付一覧に出ないため）。
            onContextMenu={
              onMenu
                ? (e) => {
                    e.preventDefault();
                    e.stopPropagation();
                    onMenu(cid, e.clientX, e.clientY);
                  }
                : undefined
            }
            className="my-2 block max-h-[480px] max-w-full rounded-md"
          />
        );
      }
    }
    // 許可して取得済みのリモート画像は、サニタイズ済み data URL で表示する。
    const remote = remoteSrc(src);
    if (remote) {
      const loaded = ctx.remoteImages[remote];
      if (loaded) {
        return (
          <RemoteImg key={key} src={loaded} alt={alt} defaultExpanded={ctx.remoteDefaultExpanded} />
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
  // blockquote の内側は 1 段深い引用として描く（子の縦線の色が変わる）。
  const childQuoteDepth = tag === 'blockquote' ? quoteDepth + 1 : quoteDepth;
  const children: ReactNode[] = [];
  el.childNodes.forEach((c, i) =>
    children.push(renderNode(c, i, ctx, insideChildLink, childQuoteDepth)),
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

  // 引用は縦線＋字下げで段を見せる（送信側 Compose も左罫線付きの blockquote で送っている）。
  if (tag === 'blockquote') {
    return (
      <QuoteBlock key={key} depth={quoteDepth}>
        {children}
      </QuoteBlock>
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
 * HTML 本文が `cid:` で参照している Content-ID を重複なく集める。
 * 「本文に埋め込まれている画像」と「本文からは参照されていない inline パート」を
 * 区別するのに使う（後者は添付一覧に出して保存できるようにする）。
 */
export function inlineCidRefs(html: string): string[] {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return [];
  }
  const cids = new Set<string>();
  doc.querySelectorAll('img').forEach((img) => {
    const src = (img.getAttribute('src') ?? '').trim();
    if (src.toLowerCase().startsWith('cid:')) {
      const cid = src.slice(4).replace(/^<|>$/g, '');
      if (cid) cids.add(cid);
    }
  });
  return [...cids];
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
/** 行頭の「>」の連なりから引用の深さを数え、記号を取り除いた本文を返す。 */
function stripQuoteMarks(line: string): { depth: number; text: string } {
  let depth = 0;
  let rest = line;
  for (;;) {
    // 「>」の前の軽い字下げ（引用符の前に空白を入れるクライアントがある）まで許す。
    const m = /^[ \t]{0,3}>[ \t]?/.exec(rest);
    if (!m) break;
    depth += 1;
    rest = rest.slice(m[0].length);
  }
  return { depth, text: rest };
}

/** 深さが同じ連続行を 1 かたまりにまとめたもの（depth 0 は引用ではない地の文）。 */
type QuoteSegment = { depth: number; text: string };

/** 本文を「深さ付きのかたまり」に切り分ける。引用が無ければ 1 かたまりだけ返る。 */
function splitByQuoteDepth(text: string): QuoteSegment[] {
  const segs: QuoteSegment[] = [];
  for (const line of text.split('\n')) {
    const { depth, text: body } = stripQuoteMarks(line);
    const last = segs[segs.length - 1];
    if (last && last.depth === depth) last.text += `\n${body}`;
    else segs.push({ depth, text: body });
  }
  return segs;
}

/**
 * 深さ付きのかたまりを入れ子の React 要素に組み直す（同じ深さは並べ、深いものは
 * QuoteBlock で包む）。戻り値は「次に処理すべき位置」で、再帰の打ち切りに使う。
 */
function buildQuoteNodes(
  segs: QuoteSegment[],
  start: number,
  depth: number,
  out: ReactNode[],
  renderText: (text: string, key: string) => ReactNode,
): number {
  let i = start;
  while (i < segs.length) {
    const seg = segs[i];
    if (seg.depth < depth) break;
    if (seg.depth === depth) {
      out.push(renderText(seg.text, `q${depth}-${i}`));
      i += 1;
      continue;
    }
    // 1 段以上深いかたまりは、まとめて 1 つの引用ブロックに入れる。
    const inner: ReactNode[] = [];
    const next = buildQuoteNodes(segs, i, depth + 1, inner, renderText);
    out.push(
      <QuoteBlock key={`q${depth}-${i}`} depth={depth}>
        {inner}
      </QuoteBlock>,
    );
    i = next;
  }
  return i;
}

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
  const renderText = (t: string, key: string) => (
    <pre key={key} className={`whitespace-pre-wrap break-words font-sans ${className}`}>
      {linkifyToNodes(t, renderEmail, re, renderDate)}
    </pre>
  );

  // 行頭の「>」は文字のまま並べず、深さごとに色の違う縦線で段を見せる
  // （引用が無い本文は従来どおり <pre> 1 枚。余計な入れ子を作らない）。
  const segs = splitByQuoteDepth(text);
  if (segs.every((s) => s.depth === 0)) return renderText(text, 'q0');

  const nodes: ReactNode[] = [];
  buildQuoteNodes(segs, 0, 0, nodes, renderText);
  return <>{nodes}</>;
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
  onInlineImageMenu,
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
  /** インライン画像の右クリック（保存/開く メニューを出す）。cid と画面座標を渡す。 */
  onInlineImageMenu?: (cid: string, x: number, y: number) => void;
  /** 検索語（複数）。本文中の一致を <mark> でハイライトする。 */
  highlight?: string[];
}) {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return <>{html}</>;
  }
  const ctx: RenderCtx = {
    inlineImages,
    remoteImages,
    remoteDefaultExpanded,
    renderEmail,
    renderDate,
    onInlineImageMenu,
    highlightRe: buildHighlightRe(highlight),
  };
  const nodes: ReactNode[] = [];
  doc.body.childNodes.forEach((c, i) => nodes.push(renderNode(c, i, ctx)));
  return (
    <div className="break-words text-sm leading-relaxed text-white/90 [&_a]:break-all">{nodes}</div>
  );
}
