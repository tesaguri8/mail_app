// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { AccountSummary } from '@bindings/AccountSummary';
import type { ComposeHandoff } from '@bindings/ComposeHandoff';
import type { MailDetail } from '@bindings/MailDetail';

/**
 * 作成画面を別ウィンドウへ移す（docs/COMPOSE.md §別ウィンドウ）。ペインの「別ウィンドウで開く」が
 * 書きかけをそのまま預けること、別ウィンドウ側が預けた内容から続きを始めることを確かめる。
 */
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (k: string) => k, i18n: { language: 'ja' } }),
}));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn() }));
const win = { minimize: vi.fn(), toggleMaximize: vi.fn(), onCloseRequested: vi.fn() };
vi.mock('@tauri-apps/api/window', () => ({ getCurrentWindow: () => win }));
const mailAttachments = vi.fn();
const mailSaveDraft = vi.fn();
vi.mock('../services/mail', () => ({
  attachmentLocalPath: vi.fn(),
  attachmentMeta: vi.fn(),
  attachmentStage: vi.fn(),
  mailAttachments: (id: number) => mailAttachments(id),
  openLocalPath: vi.fn(),
  mailDraftDiscard: vi.fn(),
  mailDraftSyncRemote: vi.fn(() => Promise.resolve()),
  mailSaveDraft: (input: unknown) => mailSaveDraft(input),
  mailSend: vi.fn(),
}));
vi.mock('../services/signatures', () => ({ signatureList: () => Promise.resolve([]) }));
const composeWindowOpen = vi.fn();
const composeWindowSaveState = vi.fn();
vi.mock('../services/composeWindow', () => ({
  composeWindowOpen: (h: ComposeHandoff) => composeWindowOpen(h),
  composeWindowSaveState: (h: ComposeHandoff) => composeWindowSaveState(h),
}));
vi.mock('./RecipientInput', () => ({
  RecipientInput: ({ value }: { value: string }) => (
    <input data-testid="recipient" value={value} readOnly />
  ),
}));

const { Compose } = await import('./Compose');

const accounts = [{ id: 1, email: 'me@example.invalid', signature_id: null }] as AccountSummary[];
const source = {
  id: 10,
  account_id: 1,
  subject: '打ち合わせ',
  from_name: '相手',
  from_address: 'them@example.invalid',
  to_addresses: 'me@example.invalid',
  reply_to: null,
  cc_addresses: null,
  message_id: '<m10@example.invalid>',
  date: '2026-11-01T10:00:00+09:00',
  body_plain: '元の本文',
  clean_body: null,
  body_html: null,
} as unknown as MailDetail;

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  localStorage.clear();
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  win.onCloseRequested.mockReturnValue(Promise.resolve(() => undefined));
  mailAttachments.mockReturnValue(Promise.resolve([]));
  mailSaveDraft.mockReturnValue(Promise.resolve(77));
  composeWindowOpen.mockReturnValue(Promise.resolve('compose-1'));
  composeWindowSaveState.mockReturnValue(Promise.resolve());
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.clearAllMocks();
});

const subjectInput = () =>
  container.querySelector<HTMLInputElement>('input[placeholder="compose.subjectPlaceholder"]');
const bodyArea = () => container.querySelector('textarea');
const button = (label: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`);

const handoff: ComposeHandoff = {
  mode: 'forward',
  source_id: 10,
  draft_id: 55,
  account_id: 1,
  to: 'friend@example.invalid, ',
  cc: '',
  bcc: 'hidden@example.invalid, ',
  subject: 'Fwd: 打ち合わせ（書きかけ）',
  body: '途中まで書いた本文',
  attachments: [{ path: '/tmp/a.pdf', source_attachment_id: null, filename: 'a.pdf', size: 10 }],
  signature_id: null,
  signature_block: '',
  dirty: true,
  unsaved: false,
};

describe('別ウィンドウの作成画面', () => {
  it('預けた書きかけから始め、転送の添付を元メールから取り直さない', async () => {
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'forward', source }}
          resume={handoff}
          windowed
          onClose={onClose}
        />
      );
    });
    expect(subjectInput()?.value).toBe('Fwd: 打ち合わせ（書きかけ）');
    expect(bodyArea()?.value).toBe('途中まで書いた本文');
    // Bcc があるので Cc/Bcc 欄を開いて始める。
    expect(
      [...container.querySelectorAll<HTMLInputElement>('[data-testid="recipient"]')].map(
        (i) => i.value
      )
    ).toEqual(['friend@example.invalid, ', '', 'hidden@example.invalid, ']);
    expect(container.textContent).toContain('a.pdf');
    expect(mailAttachments).not.toHaveBeenCalled();
    // 別ウィンドウでは「別ウィンドウで開く」を出さず、最小化・最大化・閉じるを出す。
    expect(button('compose.popOut')).toBeNull();
    expect(button('titlebar.minimize')).not.toBeNull();
    expect(button('titlebar.maximize')).not.toBeNull();
    expect(win.onCloseRequested).toHaveBeenCalled();
  });

  it('× は未保存の変更が無ければ、確認せずに閉じる', async () => {
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'forward', source }}
          resume={handoff}
          windowed
          onClose={onClose}
        />
      );
    });
    await act(async () => button('titlebar.close')?.click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('× は未保存の変更があれば保存してから閉じる（自動保存が有効なとき）', async () => {
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'forward', source }}
          resume={{ ...handoff, unsaved: true }}
          windowed
          onClose={onClose}
        />
      );
    });
    await act(async () => button('titlebar.close')?.click());
    expect(mailSaveDraft).toHaveBeenCalledWith(
      expect.objectContaining({ draft_id: 55, subject: 'Fwd: 打ち合わせ（書きかけ）' })
    );
    // 保存した内容と下書きの id で Rust の預かりを置き換える（再読み込みしても続きから）。
    expect(composeWindowSaveState).toHaveBeenCalledWith(
      expect.objectContaining({
        mode: 'forward',
        source_id: 10,
        draft_id: 77,
        body: '途中まで書いた本文',
        unsaved: false,
      })
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

describe('ペインの「別ウィンドウで開く」', () => {
  it('書きかけを預けてからペインを閉じる', async () => {
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'reply', source }}
          onClose={onClose}
        />
      );
    });
    expect(button('titlebar.minimize')).toBeNull();
    await act(async () => button('compose.popOut')?.click());
    expect(composeWindowOpen).toHaveBeenCalledTimes(1);
    expect(composeWindowOpen.mock.calls[0][0]).toMatchObject({
      mode: 'reply',
      source_id: 10,
      draft_id: null,
      account_id: 1,
      to: 'them@example.invalid, ',
      subject: 'Re: 打ち合わせ',
      body: '',
      dirty: false,
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    // 何も書いていない返信は、移すときに下書きを作らない。ペインでは預かりを更新しない。
    expect(mailSaveDraft).not.toHaveBeenCalled();
    expect(composeWindowSaveState).not.toHaveBeenCalled();
  });

  it('ウィンドウが開くのを待つ間に、保留中の自動保存で下書きを作らない', async () => {
    // 自動保存（1 秒のデバウンス）の途中で移すと、ウィンドウが開くまでの間に保存が走り、
    // 預けた内容に無い下書きができて、移した先でもう 1 通作っていた（raytrek の GUI で再現）。
    let opened: (label: string) => void = () => undefined;
    composeWindowOpen.mockImplementation(
      () => new Promise<string>((resolve) => (opened = resolve))
    );
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'new' }}
          onClose={onClose}
        />
      );
    });
    const input = subjectInput();
    if (!input) throw new Error('件名の欄が無い');
    await act(async () => {
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')?.set;
      setValue?.call(input, '書きかけ');
      input.dispatchEvent(new Event('input', { bubbles: true }));
    });
    await act(async () => button('compose.popOut')?.click());
    await act(async () => {
      await new Promise((r) => setTimeout(r, 1200));
    });
    expect(mailSaveDraft).not.toHaveBeenCalled();
    await act(async () => opened('compose-1'));
    expect(composeWindowOpen.mock.calls[0][0]).toMatchObject({
      subject: '書きかけ',
      draft_id: null,
      dirty: true,
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('ウィンドウを開けなければペインに残し、理由を出す', async () => {
    composeWindowOpen.mockImplementation(() => Promise.reject(new Error('開けない')));
    const onClose = vi.fn();
    await act(async () => {
      root.render(
        <Compose
          accounts={accounts}
          defaultAccountId={1}
          target={{ mode: 'new' }}
          onClose={onClose}
        />
      );
    });
    await act(async () => button('compose.popOut')?.click());
    expect(onClose).not.toHaveBeenCalled();
    expect(container.textContent).toContain('開けない');
  });
});
