# アイコン

`tauri.conf.json` の `bundle.icon` が参照するアイコンをここに置く。

初回は元画像（正方形 PNG, 1024x1024 推奨）を用意して生成する:

```bash
npx tauri icon ./app-icon.png
```

これで `32x32.png` / `128x128.png` / `128x128@2x.png` / `icon.icns` / `icon.ico` などが生成される。
（アイコンが無いと `tauri dev` / `tauri build` が失敗する点に注意）

## 現在の元画像

- 元画像は `app-icon.png`（1024x1024。仮アイコン「雲のある青空＋送信のつばめ」）。
- 作り方は `docs/prototypes/make_app_icon.py`（要 Pillow。リポジトリ直下で
  `python docs/prototypes/make_app_icon.py src-tauri/icons/app-icon.png` → `npx tauri icon src-tauri/icons/app-icon.png`）。
