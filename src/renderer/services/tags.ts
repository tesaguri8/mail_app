import { invoke } from '@tauri-apps/api/core';
import type { TagSummary } from '@bindings/TagSummary';

// Tauri v2 は camelCase の引数キーを snake_case の Rust 引数へ自動変換する。
export const tagList = () => invoke<TagSummary[]>('tag_list');

export const tagCreate = (name: string, color: string | null) =>
  invoke<TagSummary>('tag_create', { name, color });

export const tagUpdate = (id: number, name: string, color: string | null) =>
  invoke<void>('tag_update', { id, name, color });

export const tagDelete = (id: number) => invoke<void>('tag_delete', { id });

export const mailAddTag = (ids: number[], tagId: number) =>
  invoke<void>('mail_add_tag', { ids, tagId });

export const mailRemoveTag = (ids: number[], tagId: number) =>
  invoke<void>('mail_remove_tag', { ids, tagId });
