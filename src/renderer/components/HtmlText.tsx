import { Fragment, type ReactNode } from 'react';
import { openUrl } from '@tauri-apps/plugin-opener';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** http(s)/mailto のみ許可。javascript: などは無効化する。 */
function safeHref(href: string | null): string | null {
  if (!href) return null;
  const v = href.trim();
  if (/^(https?:|mailto:)/i.test(v)) return v;
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
// 中身を捨てる要素（スクリプト・スタイル・画像など）
const DROP = new Set(['script', 'style', 'head', 'title', 'noscript', 'iframe', 'img', 'svg']);

function renderNode(node: Node, key: number): ReactNode {
  if (node.nodeType === Node.TEXT_NODE) {
    return node.textContent ?? '';
  }
  if (node.nodeType !== Node.ELEMENT_NODE) return null;

  const el = node as Element;
  const tag = el.tagName.toLowerCase();
  if (DROP.has(tag)) return null;

  const children: ReactNode[] = [];
  el.childNodes.forEach((c, i) => children.push(renderNode(c, i)));

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
 * メールの HTML 本文を「テキスト＋リンクのみ」で安全に描画する。
 * - innerHTML は使わず DOM を走査して React 要素に変換（スクリプト実行なし）
 * - 画像/スクリプト/スタイルは描画しない（リモート画像によるトラッキング既定ブロック）
 * - リンクは下線なしの水色。クリックは外部ブラウザで開く
 */
export function HtmlText({ html }: { html: string }) {
  let doc: Document;
  try {
    doc = new DOMParser().parseFromString(html, 'text/html');
  } catch {
    return <>{html}</>;
  }
  const nodes: ReactNode[] = [];
  doc.body.childNodes.forEach((c, i) => nodes.push(renderNode(c, i)));
  return (
    <div className="break-words text-sm leading-relaxed text-white/90 [&_a]:break-all">{nodes}</div>
  );
}
