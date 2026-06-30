import { invoke } from '@tauri-apps/api/core';
import type { AutoconfigResult } from '@bindings/AutoconfigResult';
import type { AccountInput } from '@bindings/AccountInput';
import type { AccountSummary } from '@bindings/AccountSummary';

export const accountAutoconfig = (email: string) =>
  invoke<AutoconfigResult>('account_autoconfig', { email });

export const accountAdd = (input: AccountInput, password: string) =>
  invoke<AccountSummary>('account_add', { input, password });

export const accountList = () => invoke<AccountSummary[]>('account_list');

export const accountTestConnection = (host: string, port: number) =>
  invoke<void>('account_test_connection', { host, port });
