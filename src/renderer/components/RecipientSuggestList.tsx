import { useTranslation } from 'react-i18next';
import { Star, UserRound, Clock } from 'lucide-react';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';

/**
 * 宛先/検索候補のドロップダウン（住所録＝contact / 履歴＝history）。
 * 位置と幅は呼び出し側が `className`（例: "absolute left-0 top-full mt-1 w-full"）で指定する。
 * 表示専用: 選択は onPick、キーボード用のハイライトは active/onHover で親が制御する。
 * docs/RECIPIENT_AUTOCOMPLETE.md
 */
export function RecipientSuggestList({
  items,
  active,
  onPick,
  onHover,
  listId,
  className,
}: {
  items: RecipientSuggestion[];
  active: number;
  onPick: (s: RecipientSuggestion) => void;
  onHover: (i: number) => void;
  listId?: string;
  className?: string;
}) {
  const { t } = useTranslation();
  return (
    <ul
      id={listId}
      role="listbox"
      className={`z-20 max-h-64 overflow-y-auto rounded-md border border-white/15 bg-neutral-900/80 py-1 shadow-xl backdrop-blur ${
        className ?? ''
      }`}
    >
      {items.map((s, i) => (
        <li
          key={`${s.source}:${s.email}`}
          role="option"
          aria-selected={i === active}
          // blur より先に選択を確定させるため mousedown で拾う。
          onMouseDown={(e) => {
            e.preventDefault();
            onPick(s);
          }}
          onMouseEnter={() => onHover(i)}
          className={`flex cursor-pointer items-center gap-2 px-3 py-1.5 text-sm ${
            i === active ? 'bg-white/15' : ''
          }`}
        >
          {s.is_favorite ? (
            <Star size={13} className="shrink-0 fill-amber-300 text-amber-300" />
          ) : s.source === 'contact' ? (
            <UserRound size={13} className="shrink-0 text-white/45" />
          ) : (
            <Clock size={13} className="shrink-0 text-white/35" />
          )}
          <span className="min-w-0 flex-1 truncate">
            {s.name ? (
              <>
                <span className="text-white/90">{s.name}</span>{' '}
                <span className="text-white/45">&lt;{s.email}&gt;</span>
              </>
            ) : (
              <span className="text-white/90">{s.email}</span>
            )}
          </span>
          <span className="shrink-0 text-[10px] text-white/35">
            {s.source === 'contact' ? t('compose.suggestContacts') : t('compose.suggestHistory')}
          </span>
        </li>
      ))}
    </ul>
  );
}
