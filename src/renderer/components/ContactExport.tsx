import { useState } from 'react';
import { useTranslation } from 'react-i18next';
import { save } from '@tauri-apps/plugin-dialog';
import { Upload } from 'lucide-react';
import type { VcardVersion } from '@bindings/VcardVersion';
import { contactExport } from '../services/contacts';
import { APP } from '../config/appIdentity';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/** 書き出しの結果（一覧の上に出す）。 */
export type ExportOutcome = { ok: true; exported: number } | { ok: false; error: string };

/** 既定のファイル名（`rondine-contacts-YYYYMMDD.vcf`）。 */
function defaultFileName(now = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${APP.slug}-contacts-${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}.vcf`;
}

/**
 * 連絡先の書き出し（vCard）。ボタンを押すと、範囲（全員／いまの一覧）と版（3.0／4.0）を選ぶ
 * 小さなパネルを開き、保存先をダイアログで選んで書き出す。docs/IMPORT_EXPORT.md。
 *
 * - `filteredIds`: いま一覧に出ている人（検索・タグ・同期先で絞り込んでいるときだけ渡す）
 */
export function ContactExport({
  filteredIds,
  onDone,
}: {
  filteredIds: number[] | null;
  onDone: (outcome: ExportOutcome) => void;
}) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  const [scope, setScope] = useState<'all' | 'filtered'>('all');
  const [version, setVersion] = useState<VcardVersion>('3.0');
  const [busy, setBusy] = useState(false);
  const useFiltered = scope === 'filtered' && filteredIds !== null;

  const run = async () => {
    if (!isTauri || busy) return;
    let path: string | null;
    try {
      path = await save({
        defaultPath: defaultFileName(),
        filters: [{ name: 'vCard', extensions: ['vcf'] }],
      });
    } catch (e) {
      onDone({ ok: false, error: String(e) });
      return;
    }
    if (!path) return; // キャンセル
    setBusy(true);
    try {
      const report = await contactExport(path, useFiltered ? filteredIds : null, version);
      onDone({ ok: true, exported: report.exported });
      setOpen(false);
    } catch (e) {
      onDone({ ok: false, error: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const radio = (checked: boolean, onChange: () => void, label: string, disabled = false) => (
    <label className={`flex items-center gap-2 text-xs ${disabled ? 'opacity-40' : ''}`}>
      <input type="radio" checked={checked} onChange={onChange} disabled={disabled} />
      {label}
    </label>
  );

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((v) => !v)}
        title={t('contact.export')}
        aria-label={t('contact.export')}
        aria-expanded={open}
        className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-white/20 hover:bg-white/10 hover:text-white ${
          open ? 'bg-white/20 text-white' : 'text-white/70'
        }`}
      >
        <Upload size={17} />
      </button>
      {open && (
        <div className="absolute left-0 top-11 z-20 w-60 space-y-3 rounded-lg border border-white/15 bg-neutral-900/95 p-3 shadow-xl">
          <div className="text-sm font-medium">{t('contact.export')}</div>
          <div className="space-y-1.5">
            <div className="text-[11px] text-white/50">{t('contact.exportScope')}</div>
            {radio(!useFiltered, () => setScope('all'), t('contact.exportAll'))}
            {radio(
              useFiltered,
              () => setScope('filtered'),
              t('contact.exportFiltered', { count: filteredIds?.length ?? 0 }),
              filteredIds === null,
            )}
          </div>
          <div className="space-y-1.5">
            <div className="text-[11px] text-white/50">{t('contact.exportFormat')}</div>
            {radio(version === '3.0', () => setVersion('3.0'), t('contact.exportV3'))}
            {radio(version === '4.0', () => setVersion('4.0'), t('contact.exportV4'))}
          </div>
          <button
            onClick={() => void run()}
            disabled={busy}
            className="w-full rounded-md bg-sky-500/80 px-3 py-1.5 text-sm font-medium text-white hover:bg-sky-500 disabled:opacity-50"
          >
            {busy ? t('contact.exporting') : t('contact.exportRun')}
          </button>
        </div>
      )}
    </div>
  );
}
