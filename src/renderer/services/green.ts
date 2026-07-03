import { invoke } from '@tauri-apps/api/core';
import type { GreenDomainEntry } from '@bindings/GreenDomainEntry';

/** グリーン／警告ドメインの一覧（管理タブ用）。 */
export const greenDomainList = () => invoke<GreenDomainEntry[]>('green_domain_list');

/** ドメインをグリーンに認定（警告から外し手動グリーンへ）。 */
export const greenDomainAdd = (domain: string, note?: string) =>
  invoke<void>('green_domain_add', { domain, note: note ?? null });

/** ドメインを警告（グリーン解除）に。自動グリーンを上書き除外し再登録を防ぐ。 */
export const greenDomainWarn = (domain: string, note?: string) =>
  invoke<void>('green_domain_warn', { domain, note: note ?? null });

/** ドメインを中立に戻す（グリーン・警告の両方から外す）。 */
export const greenDomainClear = (domain: string) =>
  invoke<void>('green_domain_clear', { domain });

/** 単一アドレスがグリーンか。 */
export const greenAddressCheck = (address: string) =>
  invoke<boolean>('green_address_check', { address });
