import { invoke } from '@tauri-apps/api/core';
import type { SignatureSummary } from '@bindings/SignatureSummary';

export const signatureList = () => invoke<SignatureSummary[]>('signature_list');

export const signatureCreate = (name: string, body: string) =>
  invoke<SignatureSummary>('signature_create', { name, body });

export const signatureUpdate = (id: number, name: string, body: string) =>
  invoke<void>('signature_update', { id, name, body });

export const signatureDelete = (id: number) => invoke<void>('signature_delete', { id });
