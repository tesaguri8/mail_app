import { forwardRef, useImperativeHandle, useRef, useState } from 'react';
import swallowUrl from '../assets/swallow.png';

/** ビューポート座標での発着点（送信ボタン中心）。 */
export type Origin = { x: number; y: number };

export type FlySwallowHandle = {
  /**
   * つばめを発着点から飛ばし、送信(sendPromise)の完了を待って戻す。
   * 最低 1 周は必ず飛び、送信が長引けば周回を追加する。
   * 送信が失敗した場合は着地後に元の例外を再スローする。
   */
  deliver: (origin: Origin, sendPromise: Promise<unknown>) => Promise<void>;
};

const SPRITE = 116; // 飛行スプライトの基準サイズ(px)
const REDUCED =
  typeof window !== 'undefined' &&
  window.matchMedia?.('(prefers-reduced-motion: reduce)').matches;

const clamp = (v: number, a: number, b: number) => Math.max(a, Math.min(b, v));
const lerp = (a: number, b: number, t: number) => a + (b - a) * t;

/** 1 点分の transform 文字列（発着点は中心合わせのため -SPRITE/2）。 */
function tf(x: number, y: number, rot: number, scale: number, flip: number): string {
  return `translate(${x - SPRITE / 2}px, ${y - SPRITE / 2}px) rotate(${rot}deg) scale(${scale}) scaleX(${flip})`;
}

/** ウィンドウ内を 1 周する軌道（毎回ゆらぎで変化）。offset 付き keyframes を返す。 */
function roamKeyframes(origin: Origin): { frames: Keyframe[]; end: string } {
  const W = window.innerWidth;
  const H = window.innerHeight;
  const j = (n: number) => (Math.random() - 0.5) * n;
  const pts = [
    { x: origin.x, y: origin.y },
    { x: W * 0.3 + j(80), y: H * 0.5 + j(60) },
    { x: W * 0.14 + j(60), y: H * 0.26 + j(50) },
    { x: W * 0.45 + j(120), y: H * 0.14 + j(40) },
    { x: W * 0.8 + j(90), y: H * 0.2 + j(50) },
    { x: W * 0.88 + j(60), y: H * 0.48 + j(50) },
    { x: W * 0.6 + j(120), y: H * 0.4 + j(50) },
    { x: origin.x, y: origin.y },
  ];
  const times = [0, 0.16, 0.3, 0.46, 0.64, 0.8, 0.92, 1];
  const frames: Keyframe[] = [];
  let end = '';
  for (let i = 0; i < pts.length; i++) {
    const a = pts[Math.max(0, i - 1)];
    const b = pts[Math.min(pts.length - 1, i + 1)];
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy) || 1;
    const flip = dx < 0 ? 1 : -1; // 素材は左向き。右へ進むとき反転
    const bank = clamp((dy / len) * 26, -22, 22);
    const rot = dx < 0 ? bank : -bank;
    const scale = lerp(0.5, 1.05, clamp(pts[i].y / H, 0, 1)); // 上=遠い=小さい
    const transform = tf(pts[i].x, pts[i].y, rot, scale, flip);
    frames.push({ transform, offset: times[i] });
    if (i === pts.length - 1) end = transform;
  }
  return { frames, end };
}

const nextFrame = () => new Promise<void>((r) => requestAnimationFrame(() => r()));

/** WAAPI アニメを実行し、完了後に最終 transform を要素へ固定する。 */
async function run(el: HTMLElement, frames: Keyframe[], opts: KeyframeAnimationOptions, end: string) {
  const anim = el.animate(frames, opts);
  try {
    await anim.finished;
  } finally {
    el.style.transform = end;
    anim.cancel();
  }
}

/**
 * 送信時に「つばめが手紙を届けて戻る」演出。ビューポート全面のオーバーレイ。
 * Web Animations API で実装（framer-motion 等の追加依存なし）。
 */
export const FlySwallow = forwardRef<FlySwallowHandle>(function FlySwallow(_props, ref) {
  const [active, setActive] = useState(false);
  const spriteRef = useRef<HTMLDivElement>(null);
  const flapRef = useRef<HTMLDivElement>(null);

  useImperativeHandle(ref, () => ({
    async deliver(origin, sendPromise) {
      // reduced-motion: 演出を省き、送信だけ待つ
      if (REDUCED) {
        await sendPromise;
        return;
      }

      setActive(true);
      await nextFrame(); // スプライト DOM のマウントを待つ
      const sprite = spriteRef.current;
      const flap = flapRef.current;
      if (!sprite || !flap) {
        await sendPromise;
        setActive(false);
        return;
      }

      // 発着点に配置
      sprite.style.transform = tf(origin.x, origin.y, 0, 0.85, -1);

      // 羽ばたきパルス（付け根を軸に微妙な上下）
      const flapAnim = flap.animate(
        [{ transform: 'scaleY(1)' }, { transform: 'scaleY(0.9)' }, { transform: 'scaleY(1)' }],
        { duration: 160, iterations: Infinity, easing: 'ease-in-out' }
      );

      // 送信の決着を監視（成否は最後に反映）
      let settled = false;
      let err: unknown = null;
      const watch = Promise.resolve(sendPromise).then(
        () => {
          settled = true;
        },
        (e) => {
          settled = true;
          err = e;
        }
      );

      // 最低 1 周、送信が長引けば周回を足す
      do {
        const { frames, end } = roamKeyframes(origin);
        await run(sprite, frames, { duration: 2800, easing: 'ease-in-out' }, end);
      } while (!settled);
      await watch;

      // 着地（発着点へ戻ってふわっと止まる）
      await run(
        sprite,
        [
          { transform: sprite.style.transform, offset: 0 },
          { transform: tf(origin.x, origin.y, -10, 0.78, -1), offset: 0.6 },
          { transform: tf(origin.x, origin.y, 0, 0.72, -1), offset: 1 },
        ],
        { duration: 480, easing: 'ease-out' },
        tf(origin.x, origin.y, 0, 0.72, -1)
      );

      flapAnim.cancel();
      setActive(false);
      if (err) throw err;
    },
  }));

  if (!active) return null;

  return (
    <div className="pointer-events-none fixed inset-0 z-[60]" aria-hidden="true">
      <div
        ref={spriteRef}
        className="absolute left-0 top-0 will-change-transform"
        style={{ width: SPRITE, height: SPRITE }}
      >
        <div ref={flapRef} style={{ transformOrigin: '44% 42%' }}>
          <img
            src={swallowUrl}
            alt=""
            draggable={false}
            style={{
              width: '100%',
              height: 'auto',
              display: 'block',
              filter: 'drop-shadow(0 8px 12px rgba(0,0,0,.28))',
            }}
          />
        </div>
      </div>
    </div>
  );
});
