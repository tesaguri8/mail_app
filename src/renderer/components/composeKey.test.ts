import { describe, expect, it, vi } from 'vitest';
import type { MailDetail } from '@bindings/MailDetail';
import type { DraftContent } from '@bindings/DraftContent';

vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));

import { composeKey } from './Compose';

// 作成画面は対象が変わったら作り直す（<Compose key>）。返信を書きかけて下書きに差し替えたとき、
// 前の対象の空の本文が残って「下書きの編集」が空になる不具合の再発防止。
const mail = (id: number) => ({ id }) as MailDetail;
const draft = (id: number) => ({ id }) as DraftContent;

describe('composeKey', () => {
  it('返信と、その返信の下書きは別の鍵', () => {
    expect(composeKey({ mode: 'reply', source: mail(10) })).not.toBe(
      composeKey({ mode: 'draft', draft: draft(34), source: mail(10) })
    );
  });

  it('下書きは下書き id、返信・転送は種類と元メールで決まる', () => {
    expect(composeKey({ mode: 'draft', draft: draft(34) })).toBe('draft:34');
    expect(composeKey({ mode: 'draft', draft: draft(35) })).toBe('draft:35');
    expect(composeKey({ mode: 'reply', source: mail(10) })).toBe('reply:10');
    expect(composeKey({ mode: 'replyAll', source: mail(10) })).toBe('replyAll:10');
    expect(composeKey({ mode: 'forward', source: mail(11) })).toBe('forward:11');
    expect(composeKey({ mode: 'new' })).toBe('new:');
    expect(composeKey({ mode: 'new', to: 'a@example.com, ' })).toBe('new:a@example.com, ');
  });
});
