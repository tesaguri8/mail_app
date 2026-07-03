// Fly 送信演出の「羽ばたき」効果音（仮）。Web Audio で合成するのでアセット不要。
// 後日、アニメーションを整えるときに実音源へ差し替え予定（docs/FLY_SEND.md）。

let ctx: AudioContext | null = null;
function audioCtx(): AudioContext | null {
  try {
    if (!ctx) ctx = new AudioContext();
    return ctx;
  } catch {
    return null; // 音を出せない環境（テスト/ヘッドレス）では無音
  }
}

/**
 * 短い羽ばたき音を数回鳴らす（バンドパスノイズのバースト＝1回の羽ばたき）。
 * ボタンクリック（ユーザー操作）から呼ぶ前提。自動再生ポリシー対策で resume する。
 */
export function playFlySound(): void {
  const ac = audioCtx();
  if (!ac) return;
  if (ac.state === 'suspended') void ac.resume();
  const now = ac.currentTime;
  const flaps = 3;
  for (let i = 0; i < flaps; i++) {
    const t = now + i * 0.13;
    const dur = 0.12;
    // ホワイトノイズのバッファ（1回の羽ばたき分）。
    const buffer = ac.createBuffer(1, Math.max(1, Math.floor(ac.sampleRate * dur)), ac.sampleRate);
    const data = buffer.getChannelData(0);
    for (let j = 0; j < data.length; j++) data[j] = Math.random() * 2 - 1;
    const src = ac.createBufferSource();
    src.buffer = buffer;
    // バンドパスを上へスイープ＝「バサッ」という空気を切る音。
    const bp = ac.createBiquadFilter();
    bp.type = 'bandpass';
    bp.frequency.setValueAtTime(450, t);
    bp.frequency.exponentialRampToValueAtTime(1500, t + dur);
    bp.Q.value = 0.8;
    // 素早い立ち上がり→減衰の音量エンベロープ。羽ばたきごとに少し弱く。
    const g = ac.createGain();
    const peak = Math.max(0.08, 0.32 - i * 0.07);
    g.gain.setValueAtTime(0.0001, t);
    g.gain.exponentialRampToValueAtTime(peak, t + 0.02);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    src.connect(bp).connect(g).connect(ac.destination);
    src.start(t);
    src.stop(t + dur);
  }
}
