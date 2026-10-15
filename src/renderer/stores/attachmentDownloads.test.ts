import { beforeEach, describe, expect, it, vi } from 'vitest';

vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));

import {
  applyProgress,
  dismissDownload,
  downloadPercent,
  downloadsForTest,
  emailDownloadKey,
  formatElapsed,
  parseDownloadKey,
  resetDownloadsForTest,
  trackAttachment,
  type DownloadState,
} from './attachmentDownloads';

const pdf = { id: 15603, filename: '見積書.pdf', size: 816_000, emailId: 7 };

/** 外から終わらせられる約束。 */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe('trackAttachment（添付の取得の状態）', () => {
  beforeEach(() => resetDownloadsForTest());

  it('取得中は状態があり、成功したら消える', async () => {
    const d = deferred<void>();
    const p = trackAttachment(pdf, () => d.promise);
    expect(downloadsForTest().get(pdf.id)?.stage).toBe('preparing');
    d.resolve();
    await p;
    expect(downloadsForTest().has(pdf.id)).toBe(false);
  });

  it('失敗したら理由を行に残し、例外はそのまま投げ直す', async () => {
    await expect(trackAttachment(pdf, () => Promise.reject('imap.timeout'))).rejects.toBe(
      'imap.timeout'
    );
    expect(downloadsForTest().get(pdf.id)?.error).toBe('imap.timeout');
    dismissDownload(pdf.id);
    expect(downloadsForTest().has(pdf.id)).toBe(false);
  });

  it('進み具合は取得中の添付にだけ反映する', async () => {
    const d = deferred<void>();
    const p = trackAttachment(pdf, () => d.promise);
    applyProgress({ attachment_id: pdf.id, stage: 'fetching', received: 65536, total: 1_100_000 });
    const s = downloadsForTest().get(pdf.id);
    expect(s?.stage).toBe('fetching');
    expect(s?.received).toBe(65536);
    expect(s?.total).toBe(1_100_000);
    // 取得していない添付の知らせは無視する。
    applyProgress({ attachment_id: 1, stage: 'fetching', received: 1, total: 2 });
    expect(downloadsForTest().has(1)).toBe(false);
    d.resolve();
    await p;
  });

  it('同じ添付を続けて押したら 1 つの状態にまとめ、両方終わるまで残す', async () => {
    const first = deferred<void>();
    const second = deferred<void>();
    const p1 = trackAttachment(pdf, () => first.promise);
    const startedAt = downloadsForTest().get(pdf.id)?.startedAt;
    const p2 = trackAttachment(pdf, () => second.promise);
    expect(downloadsForTest().get(pdf.id)?.callers).toBe(2);
    expect(downloadsForTest().get(pdf.id)?.startedAt).toBe(startedAt);
    first.resolve();
    await p1;
    expect(downloadsForTest().has(pdf.id)).toBe(true);
    second.resolve();
    await p2;
    expect(downloadsForTest().has(pdf.id)).toBe(false);
  });

  it('失敗のあと押し直したら、新しい取得として始める', async () => {
    await trackAttachment(pdf, () => Promise.reject('x')).catch(() => {});
    const d = deferred<void>();
    const p = trackAttachment(pdf, () => d.promise);
    const s = downloadsForTest().get(pdf.id);
    expect(s?.error).toBeUndefined();
    expect(s?.callers).toBe(1);
    d.resolve();
    await p;
  });
});

describe('表示の補助', () => {
  const base: DownloadState = {
    id: 1,
    emailId: 7,
    filename: 'a.pdf',
    stage: 'fetching',
    received: 0,
    total: 0,
    startedAt: 0,
    callers: 1,
  };

  it('大きさが分かれば取得中はパーセント（99 で止める）、分からなければ null', () => {
    expect(downloadPercent({ ...base, received: 500, total: 1000 })).toBe(50);
    expect(downloadPercent({ ...base, received: 2000, total: 1000 })).toBe(99);
    expect(downloadPercent(base)).toBeNull();
    expect(downloadPercent({ ...base, stage: 'connecting', total: 1000 })).toBeNull();
    expect(downloadPercent({ ...base, stage: 'saving' })).toBe(100);
  });

  it('経過時間は m:ss', () => {
    expect(formatElapsed(42_000)).toBe('0:42');
    expect(formatElapsed(185_400)).toBe('3:05');
  });

  it('メールごとの鍵は、進み具合が変わっても同じ（始まり・終わり・失敗でだけ変わる）', () => {
    const m = new Map<number, DownloadState>([
      [1, base],
      [2, { ...base, id: 2, error: 'x', callers: 0 }],
      [3, { ...base, id: 3, emailId: 8 }],
    ]);
    const key = emailDownloadKey(m, 7);
    expect(key).toBe('1:1,2:0');
    m.set(1, { ...base, received: 999 });
    expect(emailDownloadKey(m, 7)).toBe(key);
    expect(parseDownloadKey(key)).toEqual([
      { id: 1, inFlight: true },
      { id: 2, inFlight: false },
    ]);
    expect(parseDownloadKey('')).toEqual([]);
  });
});
