import { useEffect, useRef, useState } from 'react';
import type { LucideIcon } from 'lucide-react';

export type MenuItem = {
  key: string;
  label: string;
  Icon?: LucideIcon;
  danger?: boolean;
  onClick: () => void;
};

/**
 * カーソル位置に出すコンテキストメニュー。外側クリック/Esc/スクロールで閉じる。
 * 画面端でははみ出さないよう位置を補正する。
 */
export function ContextMenu({
  x,
  y,
  header,
  items,
  onClose,
}: {
  x: number;
  y: number;
  header?: string;
  items: MenuItem[];
  onClose: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const nx = Math.min(x, window.innerWidth - width - 8);
    const ny = Math.min(y, window.innerHeight - height - 8);
    setPos({ x: Math.max(8, nx), y: Math.max(8, ny) });
  }, [x, y]);

  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    window.addEventListener('blur', onClose);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', onClose);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      style={{ left: pos.x, top: pos.y }}
      className="fixed z-50 min-w-44 overflow-hidden rounded-md border border-white/15 bg-neutral-900/95 py-1 text-sm shadow-xl backdrop-blur"
    >
      {header && (
        <div className="border-b border-white/10 px-3 py-1.5 text-xs text-white/45">{header}</div>
      )}
      {items.map((it) => (
        <button
          key={it.key}
          onClick={() => {
            it.onClick();
            onClose();
          }}
          className={`flex w-full items-center gap-2.5 px-3 py-1.5 text-left hover:bg-white/10 ${
            it.danger ? 'text-red-300 hover:bg-red-500/20' : 'text-white/85'
          }`}
        >
          {it.Icon && <it.Icon size={15} className="shrink-0 opacity-80" />}
          {it.label}
        </button>
      ))}
    </div>
  );
}
