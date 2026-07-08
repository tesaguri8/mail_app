import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { ChevronDown } from 'lucide-react';

export type DropdownOption = { value: string; label: string };

/**
 * 半透明（ブラー）背景の自前ドロップダウン。ネイティブ <select> は候補リストの背景を
 * 透過にできないため、フォームで透過リストが欲しいところに使う共通部品。
 * - 候補は fixed 配置（親の overflow に切られない）。画面下端では上向きに開く。
 * - 外側クリック / Esc / スクロール / リサイズで閉じる（ContextMenu と同系統）。
 */
export function Dropdown({
  value,
  options,
  onChange,
  className = '',
  title,
  ariaLabel,
}: {
  value: string;
  options: DropdownOption[];
  onChange: (value: string) => void;
  /** トリガーボタンの追加クラス（幅など）。 */
  className?: string;
  title?: string;
  ariaLabel?: string;
}) {
  const [open, setOpen] = useState(false);
  const btnRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number; width: number } | null>(null);

  const selected = options.find((o) => o.value === value);

  // トリガー位置から fixed 配置。画面下に入りきらなければ上向きに開く。
  useLayoutEffect(() => {
    if (!open) {
      setPos(null);
      return;
    }
    const b = btnRef.current?.getBoundingClientRect();
    if (!b) return;
    const estH = Math.min(240, options.length * 32 + 8);
    const below = window.innerHeight - b.bottom;
    const top = below >= estH || below >= b.top ? b.bottom + 4 : Math.max(8, b.top - estH - 4);
    setPos({ left: b.left, top, width: b.width });
  }, [open, options.length]);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const onDown = (e: MouseEvent) => {
      const t = e.target as Node;
      if (btnRef.current?.contains(t) || popRef.current?.contains(t)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && setOpen(false);
    // 外側のスクロール（フォーム等）では固定配置とトリガーがずれるので閉じるが、
    // リスト内部のスクロール（開いた直後の scrollIntoView 含む）では閉じない。
    const onScroll = (e: Event) => {
      if (popRef.current?.contains(e.target as Node)) return;
      setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    window.addEventListener('blur', close);
    window.addEventListener('resize', close);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('blur', close);
      window.removeEventListener('resize', close);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [open]);

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        title={title}
        aria-label={ariaLabel}
        onClick={() => setOpen((v) => !v)}
        className={`flex items-center justify-between gap-1 rounded-lg bg-white/10 px-2 py-1.5 text-sm outline-none ring-1 ring-white/10 hover:bg-white/15 focus:ring-white/30 ${className}`}
      >
        <span className="truncate">{selected?.label ?? value}</span>
        <ChevronDown size={14} className="shrink-0 text-white/50" />
      </button>
      {open && pos && (
        <div
          ref={popRef}
          style={{ left: pos.left, top: pos.top, minWidth: pos.width }}
          className="fixed z-50 max-h-60 overflow-y-auto rounded-lg bg-neutral-900/80 py-1 shadow-xl ring-1 ring-white/15 backdrop-blur-md"
        >
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              ref={o.value === value ? (el) => el?.scrollIntoView({ block: 'nearest' }) : undefined}
              onClick={() => {
                onChange(o.value);
                setOpen(false);
              }}
              className={`block w-full px-3 py-1.5 text-left text-sm hover:bg-white/15 ${
                o.value === value ? 'bg-white/10 text-white' : 'text-white/85'
              }`}
            >
              {o.label}
            </button>
          ))}
        </div>
      )}
    </>
  );
}
