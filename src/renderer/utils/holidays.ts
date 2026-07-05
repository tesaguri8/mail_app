// 日本の祝日（「国民の祝日に関する法律」2020年以降の体系に準拠）。
// 固定祝日＋ハッピーマンデー＋春分/秋分＋振替休日＋国民の休日を年ごとに算出しキャッシュ。
// カレンダーの赤字表示に使う（isHoliday('YYYY-MM-DD')）。

const cache = new Map<number, Set<string>>();
const pad = (n: number) => String(n).padStart(2, '0');
const key = (y: number, m: number, d: number) => `${y}-${pad(m)}-${pad(d)}`;

/** 月 month(1-12) の第 nth 月曜の日（1-31）。 */
function nthMonday(year: number, month: number, nth: number): number {
  const first = new Date(year, month - 1, 1).getDay(); // 0=日..6=土
  const firstMonday = 1 + ((8 - first) % 7);
  return firstMonday + (nth - 1) * 7;
}
/** 春分の日（近似式）。 */
const vernalEquinox = (y: number) => Math.floor(20.8431 + 0.242194 * (y - 1980) - Math.floor((y - 1980) / 4));
/** 秋分の日（近似式）。 */
const autumnEquinox = (y: number) => Math.floor(23.2488 + 0.242194 * (y - 1980) - Math.floor((y - 1980) / 4));

function computeYear(year: number): Set<string> {
  const base = new Set<string>();
  const add = (m: number, d: number) => base.add(key(year, m, d));

  add(1, 1); // 元日
  base.add(key(year, 1, nthMonday(year, 1, 2))); // 成人の日
  add(2, 11); // 建国記念の日
  if (year >= 2020) add(2, 23); // 天皇誕生日
  add(3, vernalEquinox(year)); // 春分の日
  add(4, 29); // 昭和の日
  add(5, 3); // 憲法記念日
  add(5, 4); // みどりの日
  add(5, 5); // こどもの日
  base.add(key(year, 7, nthMonday(year, 7, 3))); // 海の日
  add(8, 11); // 山の日
  base.add(key(year, 9, nthMonday(year, 9, 3))); // 敬老の日
  add(9, autumnEquinox(year)); // 秋分の日
  base.add(key(year, 10, nthMonday(year, 10, 2))); // スポーツの日
  add(11, 3); // 文化の日
  add(11, 23); // 勤労感謝の日

  const has = (dt: Date) => base.has(key(dt.getFullYear(), dt.getMonth() + 1, dt.getDate()));

  // 国民の休日: 前日・翌日がともに祝日の平日（日曜以外）。主に9月のシルバーウィーク。
  const nationals: string[] = [];
  for (let m = 1; m <= 12; m++) {
    const dim = new Date(year, m, 0).getDate();
    for (let d = 1; d <= dim; d++) {
      const cur = new Date(year, m - 1, d);
      if (cur.getDay() === 0 || has(cur)) continue;
      if (has(new Date(year, m - 1, d - 1)) && has(new Date(year, m - 1, d + 1))) {
        nationals.push(key(year, m, d));
      }
    }
  }
  nationals.forEach((k) => base.add(k));

  // 振替休日: 祝日が日曜のとき、その後の最初の非祝日を休日に。
  const subs: string[] = [];
  for (const k of [...base]) {
    const [yy, mm, dd] = k.split('-').map(Number);
    const dt = new Date(yy, mm - 1, dd);
    if (dt.getDay() !== 0) continue;
    const nx = new Date(dt);
    do {
      nx.setDate(nx.getDate() + 1);
    } while (base.has(key(nx.getFullYear(), nx.getMonth() + 1, nx.getDate())));
    subs.push(key(nx.getFullYear(), nx.getMonth() + 1, nx.getDate()));
  }
  subs.forEach((k) => base.add(k));

  return base;
}

/** 'YYYY-MM-DD' が日本の祝日か。 */
export function isHoliday(ds: string): boolean {
  const y = Number(ds.slice(0, 4));
  if (!Number.isFinite(y)) return false;
  let set = cache.get(y);
  if (!set) {
    set = computeYear(y);
    cache.set(y, set);
  }
  return set.has(ds);
}
