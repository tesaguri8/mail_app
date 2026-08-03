import { useEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import type { MailDetail } from '@bindings/MailDetail';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';
import { mailAttachments, mailGet } from '../services/mail';
import { attachmentImage } from '../utils/imageCache';
import { formatDateTime } from '../utils/datetime';
import { HtmlText, inlineCidRefs } from './HtmlText';
import { isImage } from './AttachedImages';

/**
 * 印刷用ドキュメントのスタイル。アプリ本体（暗い配色・全面背景画像）を持ち込まないよう、
 * 描画は非表示 iframe の中で行い、そこへこの CSS だけを流し込む。Tailwind は読み込まれない
 * ので、見た目はここで完結させる（＝紙は白地・黒文字で崩れない）。
 */
const PRINT_CSS = `
  @page { margin: 15mm; }
  * { box-sizing: border-box; }
  body {
    margin: 0;
    background: #fff;
    color: #000;
    font-family: "Yu Gothic", "Hiragino Kaku Gothic ProN", "Meiryo", system-ui, sans-serif;
    font-size: 10.5pt;
    line-height: 1.7;
    -webkit-print-color-adjust: exact;
    print-color-adjust: exact;
  }
  h1 { font-size: 14pt; margin: 0 0 6pt; }
  .head { border-bottom: 1px solid #999; padding-bottom: 8pt; margin-bottom: 12pt; }
  .row { display: flex; gap: 6pt; font-size: 9.5pt; }
  .row + .row { margin-top: 2pt; }
  .key { flex: 0 0 5.5em; color: #555; }
  .val { flex: 1 1 auto; word-break: break-word; }
  .body { word-break: break-word; }
  .body pre { white-space: pre-wrap; word-break: break-word; font-family: inherit; margin: 0; }
  img { max-width: 100%; height: auto; }
  a { color: #0645ad; }
  blockquote {
    border-left: 2px solid #ccc;
    margin: 6pt 0 6pt 2pt;
    padding-left: 8pt;
    color: #333;
  }
  table { border-collapse: collapse; max-width: 100%; }
  /* 紙・PDF ではクリックできないことがあるので、リンク先を本文に添える（下の addLinkUrls）。 */
  .linkurl { color: #555; font-size: 9pt; word-break: break-all; }
  .images { margin-top: 10pt; }
  .images figure { margin: 0 0 8pt; page-break-inside: avoid; }
  .images figcaption { font-size: 8.5pt; color: #555; margin-bottom: 2pt; }
`;

/**
 * リンクの直後にリンク先 URL を書き添える（印刷・PDF ではリンクを辿れないことがあるため）。
 * リンク文字列が URL そのもの（プレーン本文の生 URL など）のときは重複するので何もしない。
 * mailto: はアドレスがそのまま見えていることが多いので、こちらも重複チェックで弾かれる。
 */
function addLinkUrls(doc: Document): void {
  doc.querySelectorAll<HTMLAnchorElement>('a[href]').forEach((a) => {
    const href = a.getAttribute('href')?.trim() ?? '';
    if (!href) return;
    const text = (a.textContent ?? '').trim();
    const bare = href.replace(/^mailto:/i, '');
    if (!text || text === href || text === bare) return;
    const note = doc.createElement('span');
    note.className = 'linkurl';
    note.textContent = ` <${bare}>`;
    a.after(note);
  });
}

/** ドキュメント内の画像がすべて読み終わるまで待つ（最大 `timeoutMs`）。 */
function waitForImages(doc: Document, timeoutMs = 5000): Promise<void> {
  const pending = [...doc.images].filter((img) => !img.complete);
  if (pending.length === 0) return Promise.resolve();
  return new Promise((resolve) => {
    let left = pending.length;
    const done = () => {
      left -= 1;
      if (left <= 0) resolve();
    };
    pending.forEach((img) => {
      img.addEventListener('load', done, { once: true });
      img.addEventListener('error', done, { once: true });
    });
    setTimeout(resolve, timeoutMs); // 取得できない画像があっても印刷は続ける
  });
}

/**
 * メール 1 通を印刷する。描画専用の非表示 iframe を作り、そこへ印刷用の版面を差し込んでから
 * ブラウザの印刷ダイアログを開く（プリンタのほか「PDF として保存」もここから選べる）。
 *
 * マウントされている間に 1 度だけ実行し、終わったら `onDone` を呼ぶ（呼び出し側が外す）。
 * 本文の描画は画面と同じ [`HtmlText`] を使うので、サニタイズ規則や cid: 画像の解決は共通。
 */
export function PrintMail({ emailId, onDone }: { emailId: number; onDone: () => void }) {
  const { t } = useTranslation();
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [doc, setDoc] = useState<Document | null>(null);
  const [detail, setDetail] = useState<MailDetail | null>(null);
  const [attachments, setAttachments] = useState<AttachmentSummary[]>([]);
  const [images, setImages] = useState<Record<number, string>>({});
  const [inlineImages, setInlineImages] = useState<Record<string, string>>({});
  // 取得が済んだか（本文・添付メタ・画像）。揃ってから印刷ダイアログを出す。
  const [ready, setReady] = useState(false);
  const printedRef = useRef(false);

  // 本文と添付を読み、本文が参照する埋め込み画像と、本文に出ない画像を用意する。
  // 画面で見えている画像はキャッシュ済みなので、多くの場合これは即座に終わる。
  useEffect(() => {
    let alive = true;
    setReady(false);
    const load = async () => {
      const d = await mailGet(emailId);
      if (!alive) return;
      setDetail(d);
      const cids = inlineCidRefs(d.body_html ?? '');
      const list = await mailAttachments(emailId).catch(() => [] as AttachmentSummary[]);
      if (!alive) return;
      setAttachments(list);
      const referenced = new Set(cids);
      for (const a of list) {
        if (!alive) return;
        const isCid = a.content_id != null && referenced.has(a.content_id);
        if (!isCid && !isImage(a)) continue;
        try {
          const url = await attachmentImage(a.id);
          if (!alive) return;
          if (isCid) setInlineImages((prev) => ({ ...prev, [a.content_id as string]: url }));
          else setImages((prev) => ({ ...prev, [a.id]: url }));
        } catch {
          /* この 1 枚は載せずに続ける */
        }
      }
    };
    load()
      .catch(() => undefined)
      .finally(() => {
        if (alive) setReady(true);
      });
    return () => {
      alive = false;
    };
  }, [emailId]);

  // iframe の document へ印刷用スタイルを流し込む（内容は下の portal で描画する）。
  useEffect(() => {
    if (!doc) return;
    const style = doc.createElement('style');
    style.textContent = PRINT_CSS;
    doc.head.appendChild(style);
  }, [doc]);

  // 版面が揃ったら画像の読み込みを待って印刷ダイアログを開く（1 度だけ）。
  useEffect(() => {
    if (!doc || !detail || !ready || printedRef.current) return;
    const win = iframeRef.current?.contentWindow;
    if (!win) return;
    printedRef.current = true;
    // portal の描画が反映されてから測る（1 フレーム待つ）。
    const timer = setTimeout(() => {
      addLinkUrls(doc);
      void waitForImages(doc).then(() => {
        try {
          win.focus();
          win.print();
        } finally {
          onDone();
        }
      });
    }, 50);
    return () => clearTimeout(timer);
  }, [doc, detail, ready, onDone]);

  // 本文に埋め込まれない画像（写真だけのメールなど）。画面表示と同じ規準で選ぶ。
  const bodyImages = useMemo(() => {
    const referenced = new Set(inlineCidRefs(detail?.body_html ?? ''));
    return attachments.filter(
      (a) => isImage(a) && !(a.content_id && referenced.has(a.content_id)),
    );
  }, [attachments, detail?.body_html]);

  const html = detail?.body_html?.trim() ?? '';
  const text = (detail?.body_plain ?? detail?.clean_body ?? '').trim();

  return (
    <>
      <iframe
        ref={iframeRef}
        title="print"
        srcDoc="<!doctype html><html><head><meta charset='utf-8'></head><body></body></html>"
        onLoad={() => setDoc(iframeRef.current?.contentDocument ?? null)}
        // 画面には出さないが、レンダリングは必要なので display:none にはしない。
        style={{ position: 'fixed', left: '-10000px', top: 0, width: '210mm', height: '297mm' }}
        aria-hidden
      />
      {doc &&
        detail &&
        createPortal(
          <div>
            <div className="head">
              <h1>{detail.subject || t('mailbox.printNoSubject')}</h1>
              <div className="row">
                <span className="key">{t('mailbox.from')}</span>
                <span className="val">
                  {detail.from_name
                    ? `${detail.from_name} <${detail.from_address ?? ''}>`
                    : (detail.from_address ?? '')}
                </span>
              </div>
              <div className="row">
                <span className="key">{t('mailbox.to')}</span>
                <span className="val">{detail.to_addresses ?? ''}</span>
              </div>
              {detail.cc_addresses && (
                <div className="row">
                  <span className="key">{t('compose.cc')}</span>
                  <span className="val">{detail.cc_addresses}</span>
                </div>
              )}
              <div className="row">
                <span className="key">{t('mailbox.printDate')}</span>
                <span className="val">{detail.date ? formatDateTime(detail.date) : ''}</span>
              </div>
              {attachments.length > 0 && (
                <div className="row">
                  <span className="key">{t('mailbox.attachments')}</span>
                  <span className="val">{attachments.map((a) => a.filename).join('、')}</span>
                </div>
              )}
            </div>
            <div className="body">
              {html ? (
                <HtmlText html={html} inlineImages={inlineImages} />
              ) : (
                <pre>{text}</pre>
              )}
            </div>
            {bodyImages.length > 0 && (
              <div className="images">
                {bodyImages.map((a) =>
                  images[a.id] ? (
                    <figure key={a.id}>
                      <figcaption>{a.filename}</figcaption>
                      <img src={images[a.id]} alt={a.filename} />
                    </figure>
                  ) : null,
                )}
              </div>
            )}
          </div>,
          doc.body,
        )}
    </>
  );
}
