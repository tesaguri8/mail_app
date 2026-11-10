// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from 'vitest';
import {
  WHEEL_ACC_INIT,
  WHEEL_STEP_PX,
  applyZoomAction,
  clampZoom,
  fieldOf,
  keyZoomAction,
  readZoom,
  stepZoom,
  wheelStep,
  writeZoom,
  zoomFieldOfKey,
  zoomLabel,
  type WheelAcc,
  type ZoomKey,
} from './bodyZoom';

const key = (k: Partial<ZoomKey>): ZoomKey => ({
  key: '',
  code: '',
  ctrlKey: true,
  metaKey: false,
  altKey: false,
  ...k,
});

describe('clampZoom / stepZoom（倍率の丸めと上下限）', () => {
  it('10% 刻みに丸め、70%〜200% に収める', () => {
    expect(clampZoom(1.23)).toBe(1.2);
    expect(clampZoom(1.25)).toBe(1.3);
    expect(clampZoom(0.1)).toBe(0.7);
    expect(clampZoom(9)).toBe(2);
    expect(clampZoom(Number.NaN)).toBe(1);
  });

  it('1 段ずつ動き、端で止まる。足し算の誤差を残さない', () => {
    let v = 1;
    for (let i = 0; i < 3; i++) v = stepZoom(v, 1);
    expect(v).toBe(1.3);
    expect(zoomLabel(v)).toBe('130%');
    expect(stepZoom(2, 1)).toBe(2);
    expect(stepZoom(0.7, -1)).toBe(0.7);
    expect(stepZoom(0.8, -1)).toBe(0.7);
  });

  it('元に戻すは 100%', () => {
    expect(applyZoomAction(1.6, 'reset')).toBe(1);
    expect(applyZoomAction(1.6, 'in')).toBe(1.7);
    expect(applyZoomAction(1.6, 'out')).toBe(1.5);
  });
});

describe('keyZoomAction（キーの読み替え）', () => {
  it('Ctrl + "+" / "=" / テンキー + / JIS の ";" で拡大', () => {
    expect(keyZoomAction(key({ key: '+' }))).toBe('in');
    expect(keyZoomAction(key({ key: '=' }))).toBe('in');
    expect(keyZoomAction(key({ key: ';' }))).toBe('in');
    expect(keyZoomAction(key({ key: '+', code: 'NumpadAdd' }))).toBe('in');
  });

  it('Ctrl + "-" / テンキー - で縮小、"0" / テンキー 0 で元に戻す', () => {
    expect(keyZoomAction(key({ key: '-' }))).toBe('out');
    expect(keyZoomAction(key({ key: '-', code: 'NumpadSubtract' }))).toBe('out');
    expect(keyZoomAction(key({ key: '0' }))).toBe('reset');
    expect(keyZoomAction(key({ key: 'Insert', code: 'Numpad0' }))).toBe('reset');
  });

  it('mac の ⌘ でも効く', () => {
    expect(keyZoomAction(key({ key: '+', ctrlKey: false, metaKey: true }))).toBe('in');
  });

  it('修飾なし・Alt 付き・変換中・ほかのキーは何もしない', () => {
    expect(keyZoomAction(key({ key: '+', ctrlKey: false }))).toBeNull();
    expect(keyZoomAction(key({ key: '+', altKey: true }))).toBeNull();
    expect(keyZoomAction(key({ key: '+', isComposing: true }))).toBeNull();
    expect(keyZoomAction(key({ key: 's' }))).toBeNull();
  });
});

describe('wheelStep（Ctrl + ホイールの段の丸め）', () => {
  it('マウスの 1 目盛り（100px）は 1 段。上へ回すと拡大', () => {
    expect(wheelStep(WHEEL_ACC_INIT, -100, 0, 1000).step).toBe(1);
    expect(wheelStep(WHEEL_ACC_INIT, 100, 0, 1000).step).toBe(-1);
  });

  it('大きな delta でも 1 回のイベントで動くのは 1 段だけ', () => {
    const r = wheelStep(WHEEL_ACC_INIT, -1000, 0, 1000);
    expect(r.step).toBe(1);
    expect(r.next.acc).toBe(0);
  });

  it('行単位（deltaMode=1）の 3 行は 1 段', () => {
    expect(wheelStep(WHEEL_ACC_INIT, -3, 1, 1000).step).toBe(1);
  });

  it('トラックパッドの細かい delta は貯めて 1 段にする', () => {
    let acc: WheelAcc = WHEEL_ACC_INIT;
    const steps: number[] = [];
    for (let i = 0; i < 12; i++) {
      const r = wheelStep(acc, -10, 0, 1000 + i * 16);
      acc = r.next;
      steps.push(r.step);
    }
    // 10px ずつ 12 回 = 120px → 40px ごとに 1 段で 3 段（毎回ではない）。
    expect(WHEEL_STEP_PX).toBe(40);
    expect(steps.filter((s) => s !== 0)).toEqual([1, 1, 1]);
    expect(steps[0]).toBe(0);
  });

  it('向きが変わる・間が空くと貯めを捨てる', () => {
    const a = wheelStep(WHEEL_ACC_INIT, -30, 0, 1000).next;
    expect(wheelStep(a, 20, 0, 1010)).toMatchObject({ step: 0, next: { acc: 20 } });
    expect(wheelStep(a, -30, 0, 2000)).toMatchObject({ step: 0, next: { acc: -30 } });
    expect(wheelStep(a, -30, 0, 1010).step).toBe(1);
  });
});

describe('fieldOf（ポインタ位置の欄）', () => {
  it('印の付いた祖先の欄を返し、欄の外や不明な印は null', () => {
    document.body.innerHTML = `
      <div data-zoom-field="bubble" id="b"><div><p id="in">本文</p></div></div>
      <textarea data-zoom-field="compose" id="c"></textarea>
      <div data-zoom-field="other"><span id="odd"></span></div>
      <button id="out">返信</button>`;
    const $ = (id: string) => document.getElementById(id);
    expect(fieldOf($('in'))).toEqual({ field: 'bubble', el: $('b') });
    expect(fieldOf($('c'))?.field).toBe('compose');
    expect(fieldOf($('odd'))).toBeNull();
    expect(fieldOf($('out'))).toBeNull();
    expect(fieldOf(null)).toBeNull();
  });
});

describe('readZoom / writeZoom（欄ごとの保存）', () => {
  beforeEach(() => localStorage.clear());

  it('欄ごとに別の倍率を持ち、壊れた値は 100%', () => {
    writeZoom('bubble', 1.4);
    writeZoom('compose', 0.9);
    expect(readZoom('bubble')).toBe(1.4);
    expect(readZoom('compose')).toBe(0.9);
    expect(readZoom('source')).toBe(1);
    localStorage.setItem('rondine.bodyZoom.source', 'abc');
    expect(readZoom('source')).toBe(1);
  });

  it('保存のキーから欄を引ける（別ウィンドウの変更の追随に使う）', () => {
    expect(zoomFieldOfKey('rondine.bodyZoom.source')).toBe('source');
    expect(zoomFieldOfKey('rondine.dim')).toBeNull();
    expect(zoomFieldOfKey(null)).toBeNull();
  });
});
