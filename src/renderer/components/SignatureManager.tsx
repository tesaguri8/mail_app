import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ChevronDown, Minus, Plus } from 'lucide-react';
import type { SignatureSummary } from '@bindings/SignatureSummary';
import {
  signatureCreate,
  signatureDelete,
  signatureList,
  signatureUpdate,
} from '../services/signatures';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

const inputCls =
  'w-full rounded-md bg-white/10 px-3 py-2 text-sm text-white placeholder-white/40 outline-none focus:bg-white/20';
const btnCls = 'rounded-md bg-white/15 px-3 py-2 text-sm hover:bg-white/25 disabled:opacity-40';

/**
 * 署名の管理（一覧・作成・編集・削除）。アカウント編集の署名ドロップダウンの元データ。
 */
export function SignatureManager() {
  const { t } = useTranslation();
  const [items, setItems] = useState<SignatureSummary[]>([]);
  const [editing, setEditing] = useState<number | null>(null);
  const [name, setName] = useState('');
  const [body, setBody] = useState('');
  const [status, setStatus] = useState('');

  const load = () => {
    if (!isTauri) return;
    signatureList()
      .then(setItems)
      .catch(() => undefined);
  };
  useEffect(load, []);

  const toggle = (s: SignatureSummary) => {
    if (editing === s.id) {
      setEditing(null);
      return;
    }
    setEditing(s.id);
    setName(s.name);
    setBody(s.body);
    setStatus('');
  };

  const save = async (id: number) => {
    try {
      await signatureUpdate(id, name.trim(), body);
      setStatus('✓ ' + t('signature.save'));
      load();
    } catch (e) {
      setStatus('✕ ' + String(e));
    }
  };

  const add = async () => {
    try {
      const created = await signatureCreate('', '');
      setItems((prev) => [...prev, created]);
      setEditing(created.id);
      setName('');
      setBody('');
      setStatus('');
    } catch {
      /* noop */
    }
  };

  const remove = async (id: number) => {
    try {
      await signatureDelete(id);
      if (editing === id) setEditing(null);
      load();
    } catch {
      /* noop */
    }
  };

  return (
    <div className="max-w-[460px] text-left">
      {items.length === 0 ? (
        <p className="text-sm text-white/60">{t('signature.none')}</p>
      ) : (
        <ul className="space-y-2">
          {items.map((s) => (
            <li key={s.id} className="overflow-hidden rounded-md bg-white/10 text-sm">
              <div className="flex items-center justify-between gap-2 px-3 py-2">
                <button
                  onClick={() => toggle(s)}
                  className="flex min-w-0 flex-1 items-center gap-2 text-left"
                >
                  <span className="min-w-0 truncate font-medium">
                    {s.name || t('signature.untitled')}
                  </span>
                  <ChevronDown
                    size={16}
                    className={`ml-auto shrink-0 text-white/40 transition-transform ${
                      editing === s.id ? 'rotate-180' : ''
                    }`}
                  />
                </button>
                <button
                  className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-white/20 text-white/60 hover:border-red-400/60 hover:bg-red-500/30 hover:text-white"
                  title={t('signature.delete')}
                  aria-label={t('signature.delete')}
                  onClick={() => remove(s.id)}
                >
                  <Minus size={18} />
                </button>
              </div>

              {editing === s.id && (
                <div className="space-y-3 border-t border-white/10 bg-black/15 px-3 py-3">
                  <label className="block">
                    <span className="mb-1 block text-xs text-white/55">{t('signature.name')}</span>
                    <input
                      className={inputCls}
                      placeholder={t('signature.namePlaceholder')}
                      value={name}
                      onChange={(e) => setName(e.target.value)}
                    />
                  </label>
                  <label className="block">
                    <span className="mb-1 block text-xs text-white/55">{t('signature.body')}</span>
                    <textarea
                      className={`${inputCls} h-32 resize-y font-mono leading-snug`}
                      placeholder={t('signature.bodyPlaceholder')}
                      value={body}
                      onChange={(e) => setBody(e.target.value)}
                    />
                  </label>
                  <div className="flex items-center gap-3">
                    <button className={btnCls} onClick={() => save(s.id)}>
                      {t('signature.save')}
                    </button>
                    {status && <span className="text-xs text-white/70">{status}</span>}
                  </div>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}

      <button
        onClick={add}
        title={t('signature.add')}
        aria-label={t('signature.add')}
        className="mt-3 flex h-9 w-9 items-center justify-center rounded-full border border-white/20 text-white/70 hover:bg-white/10 hover:text-white"
      >
        <Plus size={18} />
      </button>
    </div>
  );
}
