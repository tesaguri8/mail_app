import { describe, expect, it } from 'vitest';
import type { ComposeHandoff } from '@bindings/ComposeHandoff';
import type { DraftContent } from '@bindings/DraftContent';
import type { MailDetail } from '@bindings/MailDetail';
import { handoffTarget } from './composeHandoff';

// 別ウィンドウは props を受け取れないので、預けた内容と読み直した元メール・下書きから対象を組み直す。
const mail = { id: 10 } as MailDetail;
const draft = { id: 34, source_id: 10 } as DraftContent;
const handoff = (mode: ComposeHandoff['mode']): ComposeHandoff => ({
  mode,
  source_id: mode === 'new' ? null : 10,
  draft_id: mode === 'draft' ? 34 : null,
  account_id: 1,
  to: '',
  cc: '',
  bcc: '',
  subject: '',
  body: '',
  attachments: [],
  signature_id: null,
  signature_block: '',
  dirty: false,
  unsaved: false,
});

describe('handoffTarget', () => {
  it('新規は元メールが無くても作れる', () => {
    expect(handoffTarget(handoff('new'), null, null)).toEqual({ mode: 'new' });
  });

  it('返信・全員返信・転送は元メールを伴う', () => {
    for (const mode of ['reply', 'replyAll', 'forward'] as const) {
      expect(handoffTarget(handoff(mode), mail, null)).toEqual({ mode, source: mail });
    }
  });

  it('返信で元メールが読めなければ出さない（引用も In-Reply-To も組めない）', () => {
    expect(handoffTarget(handoff('reply'), null, null)).toBeNull();
  });

  it('下書きの再編集は下書きを伴い、元メールは任意', () => {
    expect(handoffTarget(handoff('draft'), mail, draft)).toEqual({
      mode: 'draft',
      draft,
      source: mail,
    });
    expect(handoffTarget(handoff('draft'), null, draft)).toEqual({
      mode: 'draft',
      draft,
      source: undefined,
    });
    expect(handoffTarget(handoff('draft'), mail, null)).toBeNull();
  });
});
