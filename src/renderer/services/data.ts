import { invoke } from '@tauri-apps/api/core';
import type { DataLocation } from '@bindings/DataLocation';

/** 現在のデータ保存先と使用量。 */
export const dataLocation = () => invoke<DataLocation>('data_location');

/** データ（mail.db + 添付）を指定フォルダへ移動する（再起動不要）。 */
export const dataRelocate = (dir: string) => invoke<DataLocation>('data_relocate', { dir });

/** データを既定の場所に戻す。 */
export const dataResetLocation = () => invoke<DataLocation>('data_reset_location');
