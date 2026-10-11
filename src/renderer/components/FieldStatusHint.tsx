import { useEffect, useRef, useState } from 'react';
import { AlertTriangle } from 'lucide-react';

/**
 * 欄の横に出す状態の ⚠（赤・黄）。乗せると理由がツールチップで出て、押すと同じ理由が
 * 小さな説明で開く。重複の ⚠（ContactConflictHint）と同じ形。
 */
export function FieldStatusHint({
  tone,
  messages,
}: {
  tone: 'red' | 'amber';
  messages: readonly string[];
}) {
  const [open, setOpen] = useState(false);
  const boxRef = useRef<HTMLDivElement>(null);

  // 外側を押す・Esc で閉じる。
  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDoc);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  const text = messages.join('\n');
  const color =
    tone === 'red' ? 'text-red-300 hover:bg-red-500/15' : 'text-amber-200 hover:bg-amber-300/15';

  return (
    <div ref={boxRef} className="relative shrink-0">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={text}
        aria-label={text}
        aria-expanded={open}
        className={`flex h-7 w-7 items-center justify-center rounded-full ${color}`}
      >
        <AlertTriangle size={14} />
      </button>
      {open && (
        <div
          role="dialog"
          className="absolute right-0 top-full z-30 mt-1 w-64 space-y-1 rounded-md border border-white/15 bg-neutral-900/95 p-3 text-xs text-white/80 shadow-lg backdrop-blur"
        >
          {messages.map((m) => (
            <p key={m}>{m}</p>
          ))}
        </div>
      )}
    </div>
  );
}
