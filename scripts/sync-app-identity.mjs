// アプリ識別情報の単一ソース化（docs/APP_IDENTITY.md）
// config/app-identity.json を真実の源とし、
//   1. src/renderer/config/appIdentity.ts を生成
//   2. src-tauri/tauri.conf.json の productName / identifier / deep link のスキームを反映
// 直書き（ハードコード）を避けるため、ビルド/起動前に必ず実行する。
import { readFileSync, writeFileSync, mkdirSync, existsSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const identityPath = resolve(root, 'config/app-identity.json');
const tsOut = resolve(root, 'src/renderer/config/appIdentity.ts');
const confPath = resolve(root, 'src-tauri/tauri.conf.json');

const id = JSON.parse(readFileSync(identityPath, 'utf8'));

// TSG One のログインの戻り先のスキーム（docs/TSG_ONE.md）。本番（channel = app）は scheme のまま、
// それ以外は `<scheme>-<channel>`（開発版 rondine-dev:// / 配る版 rondine-preview://）。
// **Rust 側（services/tsg_one/identity.rs）も同じ規則で求め、試験で tauri.conf.json と突き合わせる。**
const deepLinkScheme = id.channel === 'app' ? id.scheme : `${id.scheme}-${id.channel}`;

// 1) TS 定数を生成（編集禁止バナー付き）
const ts = `// AUTO-GENERATED from config/app-identity.json — DO NOT EDIT.
// 変更は config/app-identity.json を編集し \`npm run sync:identity\` を実行。
export const APP = ${JSON.stringify(id, null, 2)} as const;
export type AppIdentity = typeof APP;
/** TSG One のログインの戻り先のスキーム（channel から求めたもの）。 */
export const DEEP_LINK_SCHEME = ${JSON.stringify(deepLinkScheme)};
`;
mkdirSync(dirname(tsOut), { recursive: true });
writeFileSync(tsOut, ts, 'utf8');

// 2) tauri.conf.json に productName / identifier を反映
if (existsSync(confPath)) {
  const conf = JSON.parse(readFileSync(confPath, 'utf8'));
  conf.productName = id.productName;
  conf.identifier = id.identifier;
  conf.plugins = conf.plugins ?? {};
  conf.plugins['deep-link'] = { desktop: { schemes: [deepLinkScheme] } };
  writeFileSync(confPath, JSON.stringify(conf, null, 2) + '\n', 'utf8');
}

console.log(
  `[sync-app-identity] ${id.productName} / ${id.identifier} / ${deepLinkScheme}:// を反映しました。`,
);
