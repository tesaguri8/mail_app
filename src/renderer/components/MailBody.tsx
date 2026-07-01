import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { open, save } from '@tauri-apps/plugin-dialog';
import { downloadDir, join } from '@tauri-apps/api/path';
import {
  BookOpen,
  ChevronDown,
  Download,
  Forward,
  Image as ImageIcon,
  Paperclip,
  Reply,
  ReplyAll,
} from 'lucide-react';
import type { MailDetail } from '@bindings/MailDetail';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import {
  attachmentExport,
  attachmentOpen,
  attachmentView,
  mailAttachments,
} from '../services/mail';
import { getInlineImages, PREFS_EVENT } from '../config/prefs';
import { HtmlText } from './HtmlText';

function formatDate(d: string | null): string {
  if (!d) return '';
  const dt = new Date(d);
  return isNaN(dt.getTime()) ? d : dt.toLocaleString();
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

const IMAGE_EXT = /\.(jpe?g|png|gif|webp|bmp|tiff?|heic|heif|avif)$/i;

/** 画像（変換すれば表示できる HEIC 等を含む）かどうか。 */
function isImage(a: AttachmentSummary): boolean {
  if (a.content_type?.toLowerCase().startsWith('image/')) return true;
  return IMAGE_EXT.test(a.filename);
}

/**
 * メール本文の表示（インライン）。Phase: プレーン本文のみ（HTML/リモート画像は
 * 後続でサニタイズ＋ブロック。docs/MAIL_SECURITY.md）。既定は引用除去後の clean_body。
 */
export function MailBody({ detail }: { detail: MailDetail }) {
  const { t } = useTranslation();
  const [showQuotes, setShowQuotes] = useState(false);
  const [note, setNote] = useState('');
  const [attachments, setAttachments] = useState<AttachmentSummary[]>([]);
  const [busyId, setBusyId] = useState<number | null>(null);
  // 本文埋め込み画像（content_id → data URL）
  const [inlineImages, setInlineImages] = useState<Record<string, string>>({});
  // 添付画像のアプリ内プレビュー（attachment id → data URL）
  const [previews, setPreviews] = useState<Record<number, string>>({});
  const [inlineEnabled, setInlineEnabled] = useState(getInlineImages());
  // 添付セクションの開閉
  const [attachmentsOpen, setAttachmentsOpen] = useState(true);
  // チェックした添付（まとめて保存用）
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const [savingAll, setSavingAll] = useState(false);
  const [attachmentsLoaded, setAttachmentsLoaded] = useState(false);

  // 設定（インライン画像の自動取得）の変更に追従する。
  useEffect(() => {
    const onPrefs = () => setInlineEnabled(getInlineImages());
    window.addEventListener(PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(PREFS_EVENT, onPrefs);
  }, []);

  // メール切り替えごとに添付メタを読み込む（本体は押下時に取得）。
  useEffect(() => {
    let active = true;
    setAttachments([]);
    setInlineImages({});
    setPreviews({});
    setAttachmentsOpen(true);
    setSelected(new Set());
    setAttachmentsLoaded(false);
    if (detail.has_attachments) {
      mailAttachments(detail.id)
        .then((a) => {
          if (active) {
            setAttachments(a);
            setAttachmentsLoaded(true);
          }
        })
        .catch(() => active && setAttachmentsLoaded(true));
    } else {
      setAttachmentsLoaded(true);
    }
    return () => {
      active = false;
    };
  }, [detail.id, detail.has_attachments]);

  const hasHtmlBody = (detail.body_html?.trim()?.length ?? 0) > 0;

  // HTML 本文＋設定オンのとき、インライン画像を取得して cid マップを作る。
  useEffect(() => {
    let active = true;
    if (!hasHtmlBody || !inlineEnabled) return;
    const targets = attachments.filter((a) => a.kind === 'inline' && a.content_id && isImage(a));
    targets.forEach((a) => {
      attachmentView(a.id)
        .then((url) => {
          if (active && a.content_id) {
            setInlineImages((m) => ({ ...m, [a.content_id as string]: url }));
          }
        })
        .catch(() => {});
    });
    return () => {
      active = false;
    };
  }, [attachments, hasHtmlBody, inlineEnabled]);

  // 添付画像をアプリ内でプレビュー表示（トグル）。HEIC も JPEG 化して表示。
  const togglePreview = async (a: AttachmentSummary) => {
    if (previews[a.id]) {
      setPreviews((m) => {
        const next = { ...m };
        delete next[a.id];
        return next;
      });
      return;
    }
    setBusyId(a.id);
    setNote('');
    try {
      const url = await attachmentView(a.id);
      setPreviews((m) => ({ ...m, [a.id]: url }));
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 「開く」: 未取得なら取得してから OS の関連アプリで開く（HEIC は変換して開く）。
  const handleOpen = async (a: AttachmentSummary) => {
    setBusyId(a.id);
    setNote('');
    try {
      await attachmentOpen(a.id);
      setAttachments((list) =>
        list.map((x) => (x.id === a.id ? { ...x, is_downloaded: true } : x)),
      );
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 「ダウンロード」: 保存先を選び（既定はダウンロードフォルダ）、その場所へ保存する。
  const handleSave = async (a: AttachmentSummary) => {
    setNote('');
    let defaultPath = a.filename;
    try {
      defaultPath = await join(await downloadDir(), a.filename);
    } catch {
      /* ダウンロードフォルダを解決できなければファイル名だけ既定にする */
    }
    const dest = await save({ defaultPath }).catch(() => null);
    if (!dest) return;
    setBusyId(a.id);
    try {
      await attachmentExport(a.id, dest);
      setNote(t('mailbox.attachmentSaved'));
    } catch (e) {
      setNote(String(e));
    } finally {
      setBusyId(null);
    }
  };

  // 一覧に出すのは本来の添付のみ（inline 画像は本文側に表示）。
  const fileAttachments = attachments.filter((a) => a.kind !== 'inline');

  const toggleOne = (id: number) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const allSelected = fileAttachments.length > 0 && selected.size === fileAttachments.length;
  const someSelected = selected.size > 0 && !allSelected;
  const toggleAll = () => {
    setSelected(allSelected ? new Set() : new Set(fileAttachments.map((a) => a.id)));
  };

  // チェックした添付をまとめて、選んだフォルダへ保存する。
  const handleSaveSelected = async () => {
    let dir: string | null = null;
    try {
      const picked = await open({ directory: true, defaultPath: await downloadDir() });
      dir = typeof picked === 'string' ? picked : null;
    } catch {
      dir = null;
    }
    if (!dir) return;
    setSavingAll(true);
    setNote('');
    const targets = fileAttachments.filter((a) => selected.has(a.id));
    let ok = 0;
    for (const a of targets) {
      try {
        await attachmentExport(a.id, await join(dir, a.filename));
        ok += 1;
      } catch {
        /* 個別の失敗はスキップ */
      }
    }
    setNote(t('mailbox.attachmentSavedN', { count: ok }));
    setSavingAll(false);
  };

  // 作成機能は後続。今はアイコン配置とフィードバックのみ。
  const composeStub = () => setNote(t('comingSoon'));
  const COMPOSE_ACTIONS = [
    { key: 'reply', Icon: Reply },
    { key: 'replyAll', Icon: ReplyAll },
    { key: 'forward', Icon: Forward },
  ] as const;

  const clean = detail.clean_body ?? '';
  const full = detail.body_plain ?? '';
  const html = detail.body_html?.trim() ?? '';
  const hasHtml = html.length > 0;
  const hasQuotedExtra = !hasHtml && full.trim().length > clean.trim().length;
  const body = showQuotes ? full : clean || full;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="border-b border-white/10 px-5 py-3">
        <div className="flex items-start justify-between gap-3">
          <h3 className="min-w-0 truncate text-base font-semibold">
            {detail.subject ?? '(no subject)'}
          </h3>
          <div className="flex shrink-0 items-center gap-1">
            {COMPOSE_ACTIONS.map(({ key, Icon }) => (
              <button
                key={key}
                onClick={composeStub}
                title={t(`compose.${key}`)}
                aria-label={t(`compose.${key}`)}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Icon size={16} />
              </button>
            ))}
            {/* 添付トグル: 転送アイコンの後に配置 */}
            {detail.has_attachments && (
              <button
                onClick={() => setAttachmentsOpen((o) => !o)}
                title={t('mailbox.attachments')}
                aria-label={t('mailbox.attachments')}
                className="flex h-8 w-8 items-center justify-center rounded-md text-white/55 hover:text-white/80"
              >
                <Paperclip size={16} />
              </button>
            )}
            {note && <span className="ml-1 text-[10px] text-white/45">{note}</span>}
          </div>
        </div>
        <div className="mt-1 text-xs text-white/50">
          <div className="flex items-baseline justify-between gap-3">
            <span className="min-w-0 truncate">
              {t('mailbox.from')}: {detail.from_address ?? '—'}
            </span>
            <span className="shrink-0">{formatDate(detail.date)}</span>
          </div>
          {detail.to_addresses && (
            <div>
              {t('mailbox.to')}: {detail.to_addresses}
            </div>
          )}
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
        {hasHtml ? (
          <HtmlText html={html} inlineImages={inlineImages} />
        ) : body.trim() ? (
          <pre className="whitespace-pre-wrap break-words font-sans text-sm leading-relaxed text-white/90">
            {body}
          </pre>
        ) : (
          <p className="text-sm text-white/40">{t('mailbox.noBody')}</p>
        )}
      </div>

      {detail.has_attachments && (
        <div className="border-t border-white/10">
          <div className="flex items-center gap-2 px-5 py-2 text-xs font-medium text-white/50">
            {fileAttachments.length > 0 && (
              <input
                type="checkbox"
                checked={allSelected}
                ref={(el) => {
                  if (el) el.indeterminate = someSelected;
                }}
                onChange={toggleAll}
                title={t('mailbox.attachmentSelectAll')}
                className="h-3.5 w-3.5 shrink-0 accent-sky-400"
              />
            )}
            <button
              onClick={() => setAttachmentsOpen((o) => !o)}
              className="flex flex-1 items-center gap-1 hover:text-white/75"
            >
              <span>
                {t('mailbox.attachments')} ({fileAttachments.length})
              </span>
              <ChevronDown
                size={14}
                className={`transition-transform ${attachmentsOpen ? '' : '-rotate-90'}`}
              />
            </button>
            {selected.size > 0 && (
              <button
                onClick={handleSaveSelected}
                disabled={savingAll}
                className="flex shrink-0 items-center gap-1 rounded-md bg-white/10 px-2 py-1 text-white/80 hover:bg-white/20 disabled:opacity-50"
              >
                <Download size={12} />
                {t('mailbox.attachmentSaveSelected', { count: selected.size })}
              </button>
            )}
          </div>
          {attachmentsOpen && fileAttachments.length === 0 && (
            <div className="px-5 pb-3 text-xs text-white/40">
              {!attachmentsLoaded
                ? t('mailbox.attachmentBusy')
                : attachments.length > 0
                  ? t('mailbox.attachmentsInlineOnly')
                  : t('mailbox.attachmentsUnfetched')}
            </div>
          )}
          {attachmentsOpen && fileAttachments.length > 0 && (
            <ul className="max-h-64 space-y-1.5 overflow-y-auto px-5 pb-3">
              {fileAttachments.map((a) => {
                const image = isImage(a);
                const preview = previews[a.id];
                return (
                  <li key={a.id} className="rounded-md bg-white/5 px-3 py-2">
                    <div className="flex items-center gap-3">
                      <input
                        type="checkbox"
                        checked={selected.has(a.id)}
                        onChange={() => toggleOne(a.id)}
                        className="h-3.5 w-3.5 shrink-0 accent-sky-400"
                      />
                      <Paperclip size={14} className="shrink-0 text-white/40" />
                      <span
                        className="min-w-0 flex-1 truncate text-sm text-white/85"
                        title={a.filename}
                      >
                        {a.filename}
                      </span>
                      <span className="shrink-0 text-xs text-white/40">{formatSize(a.size)}</span>
                      {image && (
                        <button
                          onClick={() => togglePreview(a)}
                          disabled={busyId === a.id}
                          title={preview ? t('mailbox.attachmentHide') : t('mailbox.attachmentPreview')}
                          aria-label={t('mailbox.attachmentPreview')}
                          className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                        >
                          <ImageIcon size={13} />
                        </button>
                      )}
                      <button
                        onClick={() => handleOpen(a)}
                        disabled={busyId === a.id}
                        title={t('mailbox.attachmentOpen')}
                        aria-label={t('mailbox.attachmentOpen')}
                        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                      >
                        <BookOpen size={13} />
                      </button>
                      <button
                        onClick={() => handleSave(a)}
                        disabled={busyId === a.id}
                        title={t('mailbox.attachmentDownload')}
                        aria-label={t('mailbox.attachmentDownload')}
                        className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md bg-white/10 text-white/80 hover:bg-white/20 disabled:opacity-50"
                      >
                        <Download size={13} />
                      </button>
                    </div>
                    {preview && (
                      <img
                        src={preview}
                        alt={a.filename}
                        className="mt-2 max-h-[480px] max-w-full rounded-md"
                      />
                    )}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      )}

      {hasQuotedExtra && (
        <div className="border-t border-white/10 px-5 py-2">
          <button
            className="text-xs text-sky-300 hover:underline"
            onClick={() => setShowQuotes((v) => !v)}
          >
            {showQuotes ? t('mailbox.hideQuotes') : t('mailbox.showQuotes')}
          </button>
        </div>
      )}
    </div>
  );
}
