import { invoke } from '@tauri-apps/api/core';
import type { PostalAddress } from '@bindings/PostalAddress';

// 同梱の郵便番号表で引く（住所は外部へ送らない）。docs: src-tauri/data/postal/SOURCE.md

/** 郵便番号から住所（都道府県・市区町村・町域）を引く。7 桁でなければ空。 */
export const postalLookupByCode = (code: string) =>
  invoke<PostalAddress[]>('postal_lookup_by_code', { code });

/** 住所から郵便番号の候補を引く（絞り切れなければ複数、多すぎれば空）。 */
export const postalLookupByAddress = (region: string, city: string, street: string) =>
  invoke<PostalAddress[]>('postal_lookup_by_address', { region, city, street });
