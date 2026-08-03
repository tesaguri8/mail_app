import { useEffect, useId, useRef, useState } from 'react';
import { Copy, Pencil, Trash2, User, X } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import type { RecipientSuggestion } from '@bindings/RecipientSuggestion';
import { recipientSuggest } from '../services/recipients';
import { RecipientSuggestList } from './RecipientSuggestList';
import { ContextMenu, type MenuItem } from './ContextMenu';
import { copyText } from '../utils/clipboard';

/**
 * value（カンマ区切り文字列）を「確定済みチップ」＋「編集中の下書き」に分ける。
 * 最後のカンマ以降＝いま入力中の下書き（編集可能）、それより前＝確定チップ。
 * 事前入力（返信の宛先など）は Compose 側で末尾 ", " を付けて渡すため全件チップになる。
 */
function parseValue(value: string): { chips: string[]; draft: string } {
  const parts = value.split(',');
  const draft = parts.length > 0 ? parts[parts.length - 1].replace(/^\s+/, '') : '';
  const chips = parts.slice(0, -1).map((p) => p.trim()).filter(Boolean);
  return { chips, draft };
}

/** チップ配列と下書きを value 文字列へ戻す（下書きが空なら末尾カンマ止め＝全件確定）。 */
function composeValue(chips: string[], draft: string): string {
  const head = chips.join(', ');
  if (!draft) return chips.length > 0 ? `${head}, ` : '';
  return chips.length > 0 ? `${head}, ${draft}` : draft;
}

/** "名前 <addr>" / "addr" を表示ラベル・アドレス・妥当性に分解する。 */
function parseChip(token: string): { label: string; email: string; valid: boolean } {
  const m = token.match(/^\s*(.*?)\s*<([^>]*)>\s*$/);
  if (m) {
    const name = m[1].replace(/^"|"$/g, '').trim();
    const email = m[2].trim();
    return { label: name || email, email, valid: isEmailish(email) };
  }
  const email = token.trim();
  return { label: email, email, valid: isEmailish(email) };
}

/** ざっくりしたメールアドレス形式判定（送信を止めるためではなく、警告表示用）。 */
function isEmailish(s: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(s);
}

/** アドレス部分を小文字化した重複判定キー。 */
function emailKey(token: string): string {
  return parseChip(token).email.toLowerCase();
}

/** 既存チップと重複しない（同一アドレス）トークンだけを末尾へ追加する。 */
function dedupeAppend(chips: string[], add: string[]): string[] {
  const seen = new Set(chips.map(emailKey).filter(Boolean));
  const out = [...chips];
  for (const raw of add) {
    const token = raw.trim();
    if (!token) continue;
    const key = emailKey(token);
    if (key && seen.has(key)) continue;
    if (key) seen.add(key);
    out.push(token);
  }
  return out;
}

/** 候補を入力表記へ。表示名があれば "Name <email>"、無ければ素のアドレス。 */
function formatPick(s: RecipientSuggestion): string {
  return s.name ? `${s.name} <${s.email}>` : s.email;
}

/**
 * 宛先入力（To/Cc/Bcc 共用）。各アドレスをチップ（トークン）として区切って表示し、
 * 末尾の入力欄で新しい宛先を追加できる。住所録＋過去のやり取り相手をオートコンプリート。
 * 親とは従来どおり value: string（カンマ区切り）/ onChange の契約を維持する。
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
  const { t } = useTranslation();
  const listId = useId();
  const [suggestions, setSuggestions] = useState<RecipientSuggestion[]>([]);
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  // チップの右クリックメニュー（アドレスのコピー・編集・削除）。index は対象チップ。
  const [chipMenu, setChipMenu] = useState<{ x: number; y: number; index: number } | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  // 直前に確定挿入したことを示すフラグ（挿入直後の再クエリを抑止）。
  const justPicked = useRef(false);

  const { chips, draft } = parseValue(value);

  // 入力（下書き）を 250ms デバウンスして候補取得。
  useEffect(() => {
    if (justPicked.current) {
      justPicked.current = false;
      return;
    }
    if (draft.length < 1) {
      setSuggestions([]);
      setOpen(false);
      return;
    }
    const h = setTimeout(() => {
      recipientSuggest(draft, 8)
        .then((r) => {
          setSuggestions(r);
          setActive(0);
          // 入力欄にフォーカスがある時だけ開く（返信で宛先が自動入力された起動直後などに、
          // 候補が件名欄へ勝手に重なって出るのを防ぐ）。フォーカス時は onFocus で開く。
          setOpen(r.length > 0 && document.activeElement === inputRef.current);
        })
        .catch(() => {
          setSuggestions([]);
          setOpen(false);
        });
    }, 250);
    return () => clearTimeout(h);
  }, [draft]);

  // 下書きを 1 件のチップとして確定する（Enter・カンマ・blur から呼ぶ）。
  const commitDraft = () => {
    const token = draft.trim();
    if (!token) return;
    justPicked.current = true;
    onChange(composeValue(dedupeAppend(chips, [token]), ''));
    setOpen(false);
    setSuggestions([]);
  };

  // 候補を選んでチップ確定する。重複アドレスは追加しない。
  const pick = (s: RecipientSuggestion) => {
    justPicked.current = true;
    onChange(composeValue(dedupeAppend(chips, [formatPick(s)]), ''));
    setOpen(false);
    setSuggestions([]);
    inputRef.current?.focus();
  };

  // 指定チップを削除する。
  const removeChip = (i: number) => {
    onChange(composeValue(chips.filter((_, idx) => idx !== i), draft));
    inputRef.current?.focus();
  };

  // チップの右クリックメニュー。アドレスだけのコピーを主目的にしつつ、左クリック/× と同じ
  // 操作（編集・削除）もここから行えるようにする。
  const chipMenuItems = (i: number): MenuItem[] => {
    const chip = chips[i];
    const { email, label } = parseChip(chip);
    const items: MenuItem[] = [
      {
        key: 'copy',
        label: t('compose.recipientCopyAddress'),
        Icon: Copy,
        onClick: () => void copyText(email),
      },
    ];
    // 表示名つきの宛先だけ「名前 <addr>」のコピーも出す（素のアドレスなら重複するので出さない）。
    if (label !== email) {
      items.push({
        key: 'copyWithName',
        label: t('compose.recipientCopyWithName'),
        Icon: User,
        onClick: () => void copyText(chip),
      });
    }
    items.push(
      {
        key: 'edit',
        label: t('compose.recipientEdit'),
        Icon: Pencil,
        onClick: () => editChip(i),
      },
      {
        key: 'remove',
        label: t('compose.recipientRemove'),
        Icon: Trash2,
        danger: true,
        onClick: () => removeChip(i),
      },
    );
    return items;
  };

  // 指定チップを入力欄へ戻して編集する。編集中の下書きがあれば失わないようチップ化しておく。
  const editChip = (i: number) => {
    const rest = chips.filter((_, idx) => idx !== i);
    const base = draft.trim() ? dedupeAppend(rest, [draft.trim()]) : rest;
    onChange(composeValue(base, chips[i]));
    inputRef.current?.focus();
  };

  // 入力の変化。カンマ/セミコロン/改行が含まれたら（貼り付け含む）その手前までをチップ化する。
  const onInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    const text = e.target.value;
    if (/[,;\n]/.test(text)) {
      const segs = text.split(/[,;\n]/);
      const remainder = segs.pop() ?? '';
      onChange(composeValue(dedupeAppend(chips, segs), remainder.replace(/^\s+/, '')));
      return;
    }
    onChange(composeValue(chips, text));
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    // IME 変換中（日本語入力の確定 Enter など）はキー処理を素通しする。
    if (e.nativeEvent.isComposing) return;
    if (open && suggestions.length > 0) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setActive((i) => (i + 1) % suggestions.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setActive((i) => (i - 1 + suggestions.length) % suggestions.length);
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        pick(suggestions[active]);
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setOpen(false);
        return;
      }
    }
    // 候補が閉じている時の Enter は改行/送信に流さず、下書きがあればチップ確定する。
    if (e.key === 'Enter') {
      e.preventDefault();
      commitDraft();
      return;
    }
    // 空欄で Backspace: 直前のチップを入力欄へ戻して編集できるようにする。
    if (e.key === 'Backspace' && draft.length === 0 && chips.length > 0) {
      e.preventDefault();
      editChip(chips.length - 1);
    }
  };

  return (
    <div className="relative flex-1">
      <div
        className={`${className ?? ''} flex flex-wrap items-center gap-1.5 cursor-text focus-within:bg-white/15`}
        // 余白の左クリックで入力欄へフォーカス（チップ/ボタンのクリックは巻き込まない）。
        onMouseDown={(e) => {
          if (e.button === 0 && e.target === e.currentTarget) {
            e.preventDefault();
            inputRef.current?.focus();
          }
        }}
      >
        {chips.map((chip, i) => {
          const { label, valid } = parseChip(chip);
          return (
            <span
              key={`${i}:${chip}`}
              title={valid ? chip : `${t('compose.recipientInvalid')} — ${chip}`}
              // 左クリックで入力欄へ戻して編集（× は伝播を止めて削除のみ）。
              // 右クリックはメニュー（下の onContextMenu）に任せ、編集に落とさない。
              onMouseDown={(e) => {
                if (e.button !== 0) return;
                e.preventDefault();
                editChip(i);
              }}
              // 右クリック: アドレスのコピー・編集・削除。
              onContextMenu={(e) => {
                e.preventDefault();
                e.stopPropagation();
                setChipMenu({ x: e.clientX, y: e.clientY, index: i });
              }}
              className={`inline-flex max-w-full items-center gap-1 rounded py-0.5 pl-2 pr-1 text-xs ${
                valid
                  ? 'bg-white/15 text-white/90'
                  : 'bg-amber-500/20 text-amber-100 ring-1 ring-amber-400/40'
              }`}
            >
              <span className="max-w-[14rem] truncate">{label}</span>
              <button
                type="button"
                onMouseDown={(e) => {
                  if (e.button !== 0) return;
                  e.preventDefault();
                  e.stopPropagation();
                  removeChip(i);
                }}
                title={t('compose.recipientRemove')}
                aria-label={t('compose.recipientRemove')}
                className="shrink-0 rounded p-0.5 text-white/50 hover:bg-white/20 hover:text-white"
              >
                <X size={11} />
              </button>
            </span>
          );
        })}
        <input
          ref={inputRef}
          type="text"
          className="min-w-[8ch] flex-1 bg-transparent text-sm outline-none placeholder:text-white/30"
          value={draft}
          onChange={onInput}
          onKeyDown={onKeyDown}
          onFocus={() => suggestions.length > 0 && draft.length >= 1 && setOpen(true)}
          onBlur={() => {
            // 候補クリック（onMouseDown）を先に処理させてから閉じ、離脱時は下書きをチップ確定する。
            setTimeout(() => setOpen(false), 120);
            commitDraft();
          }}
          placeholder={chips.length === 0 ? placeholder : undefined}
          autoFocus={autoFocus}
          role="combobox"
          aria-expanded={open}
          aria-controls={listId}
          aria-autocomplete="list"
        />
      </div>
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
      {chipMenu && chips[chipMenu.index] !== undefined && (
        <ContextMenu
          x={chipMenu.x}
          y={chipMenu.y}
          header={chips[chipMenu.index]}
          items={chipMenuItems(chipMenu.index)}
          onClose={() => setChipMenu(null)}
        />
      )}
    </div>
  );
}
