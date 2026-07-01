# Fly 送信演出（つばめが手紙を届ける）

**ステータス:** 本体統合済み（実写1枚モード）／本物の羽ばたき連番は後日差し替え
**目的:** 送信という日常操作を、Rondine（伊: つばめ）の世界観で「気持ちよい所有体験」に変える。送信ボタンにツバメを置き、押すとウィンドウ内を飛び回って手紙を届け、完了するとボタンへ戻って止まる。

関連: [COMPOSE.md](COMPOSE.md)（送信取消/予約/署名）/ [UI_UX_DESIGN.md](UI_UX_DESIGN.md)（全面ビジュアル方針）/ [POSITIONING.md](POSITIONING.md)（世界観）

---

## 1. コンセプトと命名

- 送信＝「ツバメを飛ばす」の比喩。ボタン表記の第一候補は **Fly**。ブランド演出のキャッチとしてイタリア語 **Volare（飛ぶ）** を併用してもよい。
- i18n 前提で「ボタンの動詞（日: 飛ばす / 英: Fly）」と「内部イベント名（例: `mail.fly`）」を分離する。文字列はハードコードせず翻訳リソースで管理（[I18N.md](I18N.md)）。

---

## 2. UX（状態機械）

| 状態 | 挙動 |
|---|---|
| **idle** | ボタン上にツバメが止まり、軽く上下して待機 |
| **flying** | クリック→ツバメが飛び立ち、ウィンドウ内を弧を描いて自由に飛ぶ（羽ばたき継続）。SMTP 送信中はここを維持 |
| **success** | 送信完了→スッとボタンへ戻り、ふわっと着地 |
| **error** | 失敗→しょんぼり戻る＋再送導線（[COMPOSE.md](COMPOSE.md) の送信取消/予約と接続） |

### オン/オフ設定
- 設定 →「表示」に **送信アニメーション（つばめ）** トグルを用意（**既定: オン**）。派手さを好まない人向けにオフにできる。
- **オフ時**は送信ボタンを通常の **「送信」** ボタンにし、つばめの止まり・飛行演出を一切出さない（送信自体は同じ `mail_send`）。
- 保存: フロントの設定（`localStorage` キー `rondine.flyAnimation`、[config/prefs.ts](../src/renderer/config/prefs.ts) の `getFlyAnimation` / `setFlyAnimation`）。変更は `rondine:prefs` イベントで通知。
- 実装済み: 設定トグル本体（[Settings.tsx](../src/renderer/components/Settings.tsx) の表示設定）＋i18n（ja/en）。※送信ボタン側での出し分けは FlyButton 統合時に接続。

**設計の肝: 送信の実時間とアニメの尺を分離する。** 最低表示時間（例 1.2〜2.8s）を設け、実送信が速すぎても一瞬で終わらせず、遅ければ飛行ループで待たせる。プロトタイプでは実送信（ダミー）と最低飛行を `Promise.all` で並走させている。

飛び方は「自由に飛ぶ」感を出すため、ベジェ的ウェイポイント＋毎回のゆらぎ（jitter）で軌道を微妙に変える。進行方向へ機首を向け（左右反転 scaleX）、上下でバンク（rotate）、奥は小さく手前は大きく（scale＝奥行き）。

---

## 3. 技術方式（採用: B 連番スプライト）

実写クオリティは**コードではなく素材で決まる**。ベクター（SVG）は写真背景と質感が合わず「おもちゃっぽく」なるため不採用。方式比較の結論:

| 方式 | 自由飛行＆帰還 | 素材 | 判定 |
|---|---|---|---|
| A. アルファ動画 | △ 軌道が焼き付く | 透過クリップ | 帰還要件に不向き |
| **B. 連番スプライト** | ◎ 経路も帰還も制御可 | 羽ばたき1周期の連番PNG | **採用** |
| C. リアルタイム3D | ◎ 動的 | リグ付きGLTF | 将来（ホーム常駐ツバメ等）で再検討 |

**B の要点:** 羽ばたきは連番フレーム、飛行軌道はコード制御。両者を分離するので「写真クオリティの羽ばたき＋自由な経路＋ボタン帰還」が両立する。フレーム素材を差し替えてもハーネスは無改修。

### 実装スタック
- **本体は Web Animations API（WAAPI）で実装**し、追加依存（framer-motion 等）を持たない。プロトタイプ（`docs/prototypes/`）のみ Framer Motion を CDN 利用。
- スプライトの translate/rotate/scale/scaleX をキーフレームで制御（[FlySwallow.tsx](../src/renderer/components/FlySwallow.tsx)）。羽ばたきは内側要素の scaleY パルスを無限反復。
- [Compose.tsx](../src/renderer/components/Compose.tsx) の送信ボタンが `getFlyAnimation()` を見て出し分け。飛行レイヤーは `<FlySwallow>`（`fixed inset-0` オーバーレイ）。
- **飛行レイヤーは最前面**（`z-[60]`・カード/本文より上）。ツバメが文字の下に潜らないようにする。
- 送信リクエストは即開始し、その完了を待つ間つばめを飛ばす（最低1周、長引けば周回追加、着地後に成否反映）。
- `prefers-reduced-motion: reduce` のときは演出を省いて送信のみ実行。
- 素材は現状「実写1枚（[assets/swallow.png](../src/renderer/assets/swallow.png)）」の滑空＋羽ばたきパルス。**本物の羽ばたきは連番素材に差し替え予定**（フレーム描画は `<img>` 連番 or スプライトシート＋CSS steps）。

---

## 4. 実写フレーム素材の入手

`frames/swallow_00.png … NN.png`（真横・頭は左向き・透過）を用意すれば差し替えるだけ。入手ルート（精度順）:

1. **3Dモデル→Blender書き出し**（最も自由・ループ完全）: Sketchfab 等で "barn swallow rigged" を入手→羽ばたき1周期を12〜16枚、透過PNGで真横レンダー。角度違いも追加可。
2. **AI動画→連番化**（後日実施予定）: Kling/Runway/Hailuo/Luma 等で「真横・カメラ固定・単色（グリーンバック推奨）でその場羽ばたき」を生成→`ffmpeg` で切り出し→背景除去（rembg）→中心合わせ→連番PNG。*image-to-video* に既存写真を入力すると見た目が一致する。
3. **実写クリップ→ロトスコープ**（手間大）。

**AI動画の注意:** 透過出力不可・非ループ・鳥が動くための位置合わせが必要＝「背景除去＋スタビライズ」が本作業。エッジのちらつきに注意。

---

## 5. プロトタイプ（技術検証）

`docs/prototypes/` に配置。ブラウザでHTMLをダブルクリックするだけで動作（Tauri/Vite 不要、React＋Framer Motion を CDN の ESM で読込）。

| ファイル | 役割 |
|---|---|
| [prototypes/fly-swallow.html](prototypes/fly-swallow.html) | 再生ハーネス。自由軌道/機首反転/バンク/奥行き/帰還/idle止まり。`USE_REAL` で「実写1枚」/「シルエット連番」を切替。実送信と最低飛行を並走 |
| [prototypes/generate_frames.py](prototypes/generate_frames.py) | ツバメ横向きシルエットの羽ばたき12連番を生成（プレースホルダ／Pillow） |
| [prototypes/process_photo.py](prototypes/process_photo.py) | 無地の空背景の写真から背景を抜く切り抜き（Pillow・青み判定でキー） |
| prototypes/frames/ | シルエット連番＋実写切り抜き `swallow_real.png` |

**検証結果:** シルエット（ベクター）は写真背景に対し「おもちゃっぽい」。実写写真の切り抜き（Pixabay の barn swallow を青空キーで抜いたもの）は質感が背景に馴染む。ただし切り抜き1枚は翼が動かないため、**本物の羽ばたきは連番素材（上記 4-1 or 4-2）が必要**。

### 主要チューニング項目（ハーネス冒頭の定数）
- `SPRITE` … 飛行スプライト基準サイズ
- `FLAP_MS` … 1フレーム表示時間（羽ばたき速度）
- 飛行 `duration` / `times` … 飛行の尺・緩急
- `buildRoam()` のウェイポイント／`jitter` … 軌道の広さと自由度
- 奥行き `scale` レンジ／`bank` 係数

---

## 6. 本実装 TODO（未着手）

- [x] 設定トグル「送信アニメーション（つばめ）」（既定オン）＋i18n（[Settings.tsx](../src/renderer/components/Settings.tsx) / [prefs.ts](../src/renderer/config/prefs.ts)）
- [x] `FlySwallow` オーバーレイ＋ [Compose.tsx](../src/renderer/components/Compose.tsx) 統合（**実写1枚モードで動作**）。`getFlyAnimation()` で出し分け（オフ→通常「送信」ボタン）。WAAPI 実装で追加依存なし
- [x] `prefers-reduced-motion` フォールバック
- [x] i18n（`compose.fly` 等・日/英）
- [ ] 実写羽ばたき連番の用意（AI動画ルートは後日）→ `assets/swallow.png` 単枚を連番に差し替え
- [ ] error 状態（送信失敗）の専用演出＋再送導線（現状は着地後にエラー表示）
- [ ] 送信取消（Undo Send）待機中の表現（飛び立つ前に留めるか等）と整合（[COMPOSE.md](COMPOSE.md)）
- [ ] idle 用の「止まりポーズ」素材（横向き飛翔とは別に用意すると自然）

---

最終更新: 2026-07-01（実写1枚モードで本体統合）
