// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { AttachmentSummary } from '@bindings/AttachmentSummary';

/**
 * 画像の「保存」「開く」のボタン（利用者報告 2026-11-06「ダウンロードボタン等がありません」）。
 * 右クリックだけでは気づけないので、吹き出しのサムネイルにも本文の埋め込み画像にも、
 * 目に見えるボタンを重ねる。押したときに親（拡大・バブルの展開・リンク）へ渡さないことも確かめる。
 */
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
// 画像は読み込み済み（メモリキャッシュにある）として描く。取得の経路は試験の対象ではない。
vi.mock('../utils/imageCache', () => ({
  peekAttachmentImage: () => 'data:image/png;base64,AAAA',
  attachmentImage: () => Promise.resolve('data:image/png;base64,AAAA'),
}));

const { AttachedImages } = await import('./AttachedImages');
const { HtmlText } = await import('./HtmlText');

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const att = (id: number, filename: string): AttachmentSummary => ({
  id,
  filename,
  content_type: 'image/jpeg',
  size: 1234,
  is_downloaded: true,
  file_path: null,
  kind: 'attachment',
  content_id: null,
});

const button = (action: 'save' | 'open', nth = 0): HTMLButtonElement => {
  const b = container.querySelectorAll<HTMLButtonElement>(`[data-image-action="${action}"]`)[nth];
  if (!b) throw new Error(`no ${action} button`);
  return b;
};

describe('吹き出しのサムネイルの保存・開く', () => {
  it('サムネイルごとに保存と開くのボタンが出て、押すとその画像で呼ばれる', () => {
    const onAction = vi.fn();
    const images = [att(1, '壁を開ける範囲.jpg'), att(2, '平面図.png')];
    act(() => root.render(<AttachedImages images={images} onAction={onAction} compact />));
    expect(container.querySelectorAll('[data-image-action="save"]')).toHaveLength(2);
    expect(container.querySelectorAll('[data-image-action="open"]')).toHaveLength(2);
    // 読み上げのラベルにファイル名が入る（どの画像のボタンか分かる）。
    expect(button('save', 1).getAttribute('aria-label')).toBe('mailbox.imageSave: 平面図.png');

    act(() => button('save', 0).click());
    act(() => button('open', 1).click());
    expect(onAction.mock.calls).toEqual([
      [images[0], 'save'],
      [images[1], 'open'],
    ]);
  });

  it('ボタンを押しても、サムネイルは拡大しないし、バブルのクリックへも伝わらない', () => {
    const onBubble = vi.fn();
    act(() =>
      root.render(
        <div onClick={onBubble}>
          <AttachedImages images={[att(1, 'a.jpg')]} onAction={() => undefined} compact />
        </div>
      )
    );
    act(() => button('save').click());
    expect(onBubble).not.toHaveBeenCalled();
    // 拡大すると実寸のクラス（cursor-zoom-out）に変わる。変わっていないこと。
    expect(container.querySelector('img')?.className).toContain('cursor-zoom-in');
  });

  it('ボタンはホバーしていなくても薄く見えている（隠しきらない）', () => {
    act(() =>
      root.render(<AttachedImages images={[att(1, 'a.jpg')]} onAction={() => undefined} compact />)
    );
    const bar = button('save').parentElement as HTMLElement;
    expect(bar.className).toContain('opacity-60');
    expect(bar.className).not.toContain('opacity-0');
  });

  it('onAction が無ければボタンを出さない（右クリックだけの呼び出し元は従来どおり）', () => {
    act(() => root.render(<AttachedImages images={[att(1, 'a.jpg')]} compact />));
    expect(container.querySelector('[data-image-action]')).toBeNull();
  });

  it('全文表示（compact でない）にもボタンが出る', () => {
    const onAction = vi.fn();
    act(() => root.render(<AttachedImages images={[att(7, 'b.jpg')]} onAction={onAction} />));
    act(() => button('open').click());
    expect(onAction).toHaveBeenCalledWith(expect.objectContaining({ id: 7 }), 'open');
  });
});

describe('本文の埋め込み画像の保存・開く', () => {
  const html = '<p>図面です<img src="cid:img1@x.invalid" alt="図面.jpg"></p>';
  const inlineImages = { 'img1@x.invalid': 'data:image/png;base64,AAAA' };

  it('表示できた埋め込み画像にボタンが出て、押すと cid で呼ばれる', () => {
    const onAction = vi.fn();
    act(() =>
      root.render(
        <HtmlText html={html} inlineImages={inlineImages} onInlineImageAction={onAction} />
      )
    );
    expect(button('save').getAttribute('aria-label')).toBe('mailbox.imageSave: 図面.jpg');
    act(() => button('save').click());
    act(() => button('open').click());
    expect(onAction.mock.calls).toEqual([
      ['img1@x.invalid', 'save'],
      ['img1@x.invalid', 'open'],
    ]);
  });

  it('リンクの中の画像でも、ボタンはリンクを開かない', () => {
    const onLink = vi.fn();
    act(() =>
      root.render(
        <div onClick={onLink}>
          <HtmlText
            html={'<a href="https://example.invalid/"><img src="cid:img1@x.invalid" alt=""></a>'}
            inlineImages={inlineImages}
            onInlineImageAction={() => undefined}
          />
        </div>
      )
    );
    act(() => button('open').click());
    expect(onLink).not.toHaveBeenCalled();
  });

  it('読み込めていない埋め込み画像（プレースホルダ）にはボタンを出さない', () => {
    act(() =>
      root.render(<HtmlText html={html} inlineImages={{}} onInlineImageAction={() => undefined} />)
    );
    expect(container.querySelector('[data-image-action]')).toBeNull();
    expect(container.textContent).toContain('🖼');
  });
});
