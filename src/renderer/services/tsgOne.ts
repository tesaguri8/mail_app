import { invoke } from '@tauri-apps/api/core';
import type { TsgOneProvider } from '@bindings/TsgOneProvider';
import type { TsgOneStatus } from '@bindings/TsgOneStatus';

// TSG One のアカウント（Google の接続とは別。src-tauri/src/services/tsg_one.rs）。
// トークンは Rust 側だけが持ち、画面へは渡さない（状態とログインした人だけ）。

/** ブラウザから戻ってログインが済んだ・断られたときのイベント（TsgOneStatus を載せる）。 */
export const TSG_ONE_CHANGED_EVENT = 'tsg-one:changed';

export const tsgOneStatus = () => invoke<TsgOneStatus>('tsg_one_status');

/** ログインを始め、システムのブラウザで開く。戻りは TSG_ONE_CHANGED_EVENT で届く。 */
export const tsgOneLogin = (provider: TsgOneProvider) =>
  invoke<TsgOneStatus>('tsg_one_login', { provider });

export const tsgOneCancelLogin = () => invoke<TsgOneStatus>('tsg_one_cancel_login');

export const tsgOneLogout = () => invoke<TsgOneStatus>('tsg_one_logout');

/** この端末を登録し直し、台数の枠を確かめ直す（会員サイトでほかの端末を解除したあと）。 */
export const tsgOneRecheckDevice = () => invoke<TsgOneStatus>('tsg_one_recheck_device');

/** 会員サイト（TSG One のブラウザ）を開く。端末の解除など、重い操作はそこで行う。 */
export const tsgOneOpenMemberSite = () => invoke<void>('tsg_one_open_member_site');
