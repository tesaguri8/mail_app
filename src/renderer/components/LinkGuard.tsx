// 本文のリンク（HTML の <a> と、本文の URL を自動でリンクにしたもの）を、差出人の信頼に応じて
// 開かせたり止めたりする（docs/MAIL_SECURITY.md §2.1）。
// - 'open'   : 左クリックで外部ブラウザで開く（いまどおり）。
// - 'guarded': 左クリックでは開かず「右クリックで開けます」を一瞬出す。見た目は控えめにする。
// どちらも右クリックで「行き先の URL ／リンクを開く／リンクをコピー」のメニューを出す。
import { createContext, useContext, useEffect, useState, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import { openUrl } from '@tauri-apps/plugin-opener';
import { Copy, ExternalLink } from 'lucide-react';
import { ContextMenu } from './ContextMenu';
import { copyText } from '../utils/clipboard';
import { displayUrl, type LinkTrust } from './linkTrust';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 外部ブラウザで開く（Tauri の外＝レンダラ単体では新しいタブ）。 */
export function openExternal(url: string) {
  if (isTauri) openUrl(url).catch(() => undefined);
  else window.open(url, '_blank', 'noopener,noreferrer');
}

/**
 * いま描いている本文のリンクの扱い。メールの本文を描く側（会話のバブル・全文表示）が
 * 差出人ごとに LinkGuard で包む。包まれていない場所（印刷・カレンダーの説明など、
 * 自分で書いた文章）はいまどおり開く。
 */
const LinkTrustContext = createContext<LinkTrust>('open');

/** 子の本文のリンクを、差出人の信頼（linkTrust の結果）で扱う。 */
export function LinkGuard({ trust, children }: { trust: LinkTrust; children: ReactNode }) {
  return <LinkTrustContext.Provider value={trust}>{children}</LinkTrustContext.Provider>;
}

/** 止めたリンクを左クリックしたときの案内を出しておく時間（ms）。 */
const HINT_MS = 1800;

/** 止めたリンクの見た目: 白の 50%・点線の下線（開けないと分かるように。水色にしない）。 */
const GUARDED_CLASS =
  'cursor-default text-white/50 underline decoration-dotted decoration-white/40 underline-offset-2 break-all';

/**
 * 本文のリンク 1 つ。`className` は開くときの見た目（HTML 本文とプレーン本文で少し違う）。
 * クリックは親（バブルの展開トグル等）へ伝播させない。
 */
export function BodyLink({
  href,
  className,
  children,
}: {
  href: string;
  className: string;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const trust = useContext(LinkTrustContext);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const [hint, setHint] = useState<{ x: number; y: number } | null>(null);

  useEffect(() => {
    if (!hint) return;
    const timer = window.setTimeout(() => setHint(null), HINT_MS);
    return () => window.clearTimeout(timer);
  }, [hint]);

  const guarded = trust === 'guarded';
  return (
    <>
      <a
        href={href}
        title={href}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          if (guarded) setHint({ x: e.clientX, y: e.clientY });
          else openExternal(href);
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          e.stopPropagation();
          setHint(null);
          setMenu({ x: e.clientX, y: e.clientY });
        }}
        className={guarded ? GUARDED_CLASS : className}
      >
        {children}
      </a>
      {/* 案内とメニューは body へ出す（バブルの backdrop-blur の内側だと fixed の基準がずれるため）。 */}
      {hint &&
        createPortal(
          <span
            role="status"
            style={{ left: hint.x + 12, top: hint.y + 12 }}
            className="pointer-events-none fixed z-50 rounded bg-neutral-900/95 px-2 py-1 text-xs text-white/80 shadow-lg"
          >
            {t('link.guardedHint')}
          </span>,
          document.body
        )}
      {menu &&
        createPortal(
          <ContextMenu
            x={menu.x}
            y={menu.y}
            header={displayUrl(href)}
            onClose={() => setMenu(null)}
            items={[
              {
                key: 'open',
                label: t('link.open'),
                Icon: ExternalLink,
                onClick: () => openExternal(href),
              },
              {
                key: 'copy',
                label: t('link.copy'),
                Icon: Copy,
                onClick: () => {
                  copyText(href).catch(() => undefined);
                },
              },
            ]}
          />,
          document.body
        )}
    </>
  );
}
