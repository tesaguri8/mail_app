import { useEffect, useRef, useState, type ReactNode } from 'react';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * 過去入力などを候補に出す汎用オートコンプリート入力。
 * `fetch(query)` で候補文字列を取得し、ドロップダウンから選べる。業務ロジックは持たず、
 * テキスト編集と候補選択を親へ通知するだけ（DRY: 予定のタイトル欄・場所欄で共用）。
 * 候補が無い/非 Tauri 環境では素の入力欄として振る舞う。
 */
export function SuggestInput({
  value,
  onChange,
  suggest,
  placeholder,
  className,
  icon,
  autoFocus,
  ariaLabel,
  onEnter,
  max = 8,
}: {
  value: string;
  onChange: (v: string) => void;
  /** query（trim 済み）に対する候補一覧を返す。空 query なら「よく使う候補」を返してよい。 */
  suggest: (query: string) => Promise<string[]>;
  placeholder?: string;
  /** input 要素の className（呼び出し側のフォーム様式に合わせる）。 */
  className?: string;
  /** 候補行の先頭に出すアイコン（任意）。 */
  icon?: ReactNode;
  autoFocus?: boolean;
  ariaLabel?: string;
  /** 候補未選択のまま Enter を押したときの処理（任意）。 */
  onEnter?: () => void;
  max?: number;
}) {
  const [results, setResults] = useState<string[]>([]);
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(-1);
  const boxRef = useRef<HTMLDivElement>(null);

  // 開いている間、入力に応じて候補を取得（軽いデバウンス）。現在値と同一の候補は除く。
  useEffect(() => {
    if (!isTauri || !open) return;
    let alive = true;
    const h = setTimeout(() => {
      suggest(value.trim())
        .then((r) => {
          if (!alive) return;
          const cur = value.trim().toLowerCase();
          setResults(r.filter((s) => s.trim().toLowerCase() !== cur).slice(0, max));
          setHighlight(-1);
        })
        .catch(() => alive && setResults([]));
    }, 120);
    return () => {
      alive = false;
      clearTimeout(h);
    };
    // suggest は呼び出し側で安定な参照を渡す前提（インライン生成でも挙動は同じ）。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, open, max]);

  // 外側クリックで閉じる。
  useEffect(() => {
    const onDoc = (e: MouseEvent) => {
      if (boxRef.current && !boxRef.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', onDoc);
    return () => document.removeEventListener('mousedown', onDoc);
  }, []);

  const pick = (s: string) => {
    onChange(s);
    setOpen(false);
    setHighlight(-1);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open && e.key === 'ArrowDown') {
      setOpen(true);
      return;
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setHighlight((h) => Math.min(h + 1, results.length - 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(h - 1, 0));
    } else if (e.key === 'Enter') {
      if (open && highlight >= 0 && results[highlight]) {
        e.preventDefault();
        pick(results[highlight]);
      } else {
        onEnter?.();
      }
    } else if (e.key === 'Escape') {
      // 候補が開いていれば候補だけ閉じる（親の Esc＝エディタを閉じる、へは伝播させない）。
      if (open) {
        e.stopPropagation();
        setOpen(false);
      }
    }
  };

  return (
    <div ref={boxRef} className="relative flex-1">
      <input
        autoFocus={autoFocus}
        className={className}
        value={value}
        placeholder={placeholder}
        aria-label={ariaLabel}
        role="combobox"
        aria-expanded={open}
        aria-autocomplete="list"
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
          setHighlight(-1);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
      />
      {open && results.length > 0 && (
        <ul className="absolute left-0 right-0 top-full z-30 mt-1 max-h-56 overflow-y-auto rounded-md border border-white/15 bg-[#141a2e] py-1 shadow-xl">
          {results.map((s, i) => (
            <li key={s}>
              <button
                type="button"
                onMouseDown={(e) => {
                  e.preventDefault();
                  pick(s);
                }}
                onMouseEnter={() => setHighlight(i)}
                className={`flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm ${
                  i === highlight ? 'bg-white/15' : 'hover:bg-white/10'
                }`}
              >
                {icon}
                <span className="min-w-0 flex-1 truncate">{s}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
