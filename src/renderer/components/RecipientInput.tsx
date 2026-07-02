import { useEffect, useId, useRef, useState } from 'react';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';
import { recipientSuggest } from '../services/recipients';
import { RecipientSuggestList } from './RecipientSuggestList';

/** 入力中の最後のトークン（最後の , / 改行以降）と、それ以前（確定済み）を分ける。 */
function splitLastToken(value: string): { prefix: string; token: string } {
  const sep = Math.max(value.lastIndexOf(','), value.lastIndexOf('\n'));
  return { prefix: value.slice(0, sep + 1), token: value.slice(sep + 1).trim() };
}

/** 候補を入力表記へ。表示名があれば "Name <email>"、無ければ素のアドレス。 */
function formatPick(s: RecipientSuggestion): string {
  return s.name ? `${s.name} <${s.email}>` : s.email;
}

/**
 * 宛先入力（To/Cc/Bcc 共用）。カンマ区切り文字列の value/onChange 契約を保ちつつ、
 * 最後のトークンで住所録＋過去のやり取り相手をオートコンプリートする。
 * docs/RECIPIENT_AUTOCOMPLETE.md
 */
export function RecipientInput({
  value,
  onChange,
  placeholder,
  autoFocus,
  className,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
}) {
  const listId = useId();
  const [suggestions, setSuggestions] = useState<RecipientSuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  // 直前に確定挿入したことを示すフラグ（挿入直後の再クエリを抑止）。
  const justPicked = useRef(false);

  const { prefix, token } = splitLastToken(value);

  // 入力（最後のトークン）を 250ms デバウンスして候補取得。
  useEffect(() => {
    if (justPicked.current) {
      justPicked.current = false;
      return;
    }
    if (token.length < 1) {
      setSuggestions([]);
      setOpen(false);
      return;
    }
    const h = setTimeout(() => {
      recipientSuggest(token, 8)
        .then((r) => {
          setSuggestions(r);
          setActive(0);
          setOpen(r.length > 0);
        })
        .catch(() => {
          setSuggestions([]);
          setOpen(false);
        });
    }, 250);
    return () => clearTimeout(h);
  }, [token]);

  const pick = (s: RecipientSuggestion) => {
    const base = prefix ? prefix.replace(/\s*$/, '') + ' ' : '';
    justPicked.current = true;
    onChange(base + formatPick(s) + ', ');
    setOpen(false);
    setSuggestions([]);
    inputRef.current?.focus();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!open || suggestions.length === 0) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((i) => (i + 1) % suggestions.length);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((i) => (i - 1 + suggestions.length) % suggestions.length);
    } else if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault();
      pick(suggestions[active]);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setOpen(false);
    }
  };

  return (
    <div className="relative flex-1">
      <input
        ref={inputRef}
        className={className}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={onKeyDown}
        onFocus={() => suggestions.length > 0 && token.length >= 1 && setOpen(true)}
        onBlur={() => setTimeout(() => setOpen(false), 120)}
        placeholder={placeholder}
        autoFocus={autoFocus}
        role="combobox"
        aria-expanded={open}
        aria-controls={listId}
        aria-autocomplete="list"
      />
      {open && suggestions.length > 0 && (
        <RecipientSuggestList
          items={suggestions}
          active={active}
          onPick={pick}
          onHover={setActive}
          listId={listId}
          className="absolute left-0 top-full mt-1 w-full"
        />
      )}
    </div>
  );
}
