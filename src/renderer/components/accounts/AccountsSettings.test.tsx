// @vitest-environment happy-dom
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { createRoot, type Root } from 'react-dom/client';
import { act } from 'react-dom/test-utils';
import type { AccountProfile } from '@bindings/AccountProfile';

/**
 * 設定の「アカウント」: 1 枚のカードを開いている間は、そのカードだけを見せる（利用者の要望 2026-10-22）。
 * カードの中身（同期・Google の部品）は試験の対象ではないので、見出しの開閉と入力欄だけの代役にする。
 */
vi.mock('react-i18next', () => ({ useTranslation: () => ({ t: (k: string) => k }) }));
vi.mock('./shared', async (importActual) => ({
  ...(await importActual<typeof import('./shared')>()),
  isTauri: true,
}));
const profiles: AccountProfile[] = [1, 2, 3].map((id) => ({
  id,
  provider: 'imap',
  email: `user${id}@example.invalid`,
  display_name: null,
  mail_account_ids: [],
  google_account_id: null,
}));
vi.mock('../../services/accounts', () => ({
  accountProfiles: () => Promise.resolve(profiles),
  serverAccountList: () => Promise.resolve([]),
  accountProfileReorder: () => Promise.resolve(),
  accountCheck: () => Promise.resolve(),
  accountPing: () => Promise.resolve(),
}));
vi.mock('../../services/google', () => ({
  googleAccounts: () => Promise.resolve([]),
  googleCredentialsStatus: () => Promise.resolve(null),
}));
vi.mock('../../services/signatures', () => ({ signatureList: () => Promise.resolve([]) }));
vi.mock('./AccountCard', () => ({
  AccountCard: ({
    profile,
    open,
    onToggleOpen,
  }: {
    profile: AccountProfile;
    open: boolean;
    onToggleOpen: () => void;
  }) => (
    <section data-card={profile.email}>
      <button aria-expanded={open} onClick={onToggleOpen}>
        {profile.email}
      </button>
      {open && <input aria-label={`name-${profile.id}`} />}
    </section>
  ),
}));
vi.mock('./AddAccountFlow', () => ({ AddAccountFlow: () => <div data-add-flow /> }));
vi.mock('./GoogleCredentialsPanel', () => ({
  GoogleCredentialsPanel: () => (
    <div data-google-creds>
      <input aria-label="client-id" />
    </div>
  ),
}));

const { AccountsSettings } = await import('./AccountsSettings');

let container: HTMLDivElement;
let root: Root;

beforeEach(async () => {
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root.render(<AccountsSettings accounts={[]} onChanged={() => undefined} />);
  });
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

const cards = () =>
  [...container.querySelectorAll('[data-card]')].map((c) => c.getAttribute('data-card'));
const header = (email: string) =>
  container.querySelector<HTMLButtonElement>(`[data-card="${email}"] button`);
const button = (label: string) =>
  [...container.querySelectorAll('button')].find((b) => b.textContent === label);
const click = (el: HTMLElement | null | undefined) =>
  act(() => {
    el?.click();
  });
/** Tab で移れる要素（隠れたものは描いていないので、ここに無ければ Tab でも入らない）。 */
const tabbable = () =>
  [...container.querySelectorAll<HTMLElement>('button, input, a[href], [tabindex]')]
    .filter(
      (el) =>
        !el.closest('[hidden]') &&
        !el.hasAttribute('disabled') &&
        el.getAttribute('tabindex') !== '-1'
    )
    .map((el) => el.textContent || el.getAttribute('aria-label'));

const ALL = ['user1@example.invalid', 'user2@example.invalid', 'user3@example.invalid'];

describe('アカウントのカードを 1 枚開いたとき', () => {
  it('閉じている間は、全部のカードと「アカウントを追加」・Google の接続設定を出す', () => {
    expect(cards()).toEqual(ALL);
    expect(button('account.addAccount')).toBeDefined();
    expect(container.querySelector('[data-google-creds]')).not.toBeNull();
    expect(button('account.backToAll')).toBeUndefined();
  });

  it('開くと、そのカードだけを出し、上に「すべてのアカウント」を出す', () => {
    click(header('user2@example.invalid'));
    expect(cards()).toEqual(['user2@example.invalid']);
    expect(button('account.addAccount')).toBeUndefined();
    expect(container.querySelector('[data-google-creds]')).toBeNull();
    const back = button('account.backToAll');
    expect(back).toBeDefined();
    // 戻るリンクはカードより上
    const card = container.querySelector('[data-card]');
    expect(back!.compareDocumentPosition(card!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('Tab で移れるのは、戻るリンクと開いたカードの中だけ', () => {
    click(header('user2@example.invalid'));
    expect(tabbable()).toEqual(['account.backToAll', 'user2@example.invalid', 'name-2']);
  });

  it('カードの見出しで閉じると、一覧に戻る', () => {
    click(header('user2@example.invalid'));
    click(header('user2@example.invalid'));
    expect(cards()).toEqual(ALL);
    expect(button('account.backToAll')).toBeUndefined();
    expect(button('account.addAccount')).toBeDefined();
  });

  it('「すべてのアカウント」で一覧に戻り、カードは閉じている', () => {
    click(header('user3@example.invalid'));
    click(button('account.backToAll'));
    expect(cards()).toEqual(ALL);
    expect(
      [...container.querySelectorAll('[aria-expanded="true"]')].map((b) => b.textContent)
    ).toEqual([]);
    expect(container.querySelector('[data-google-creds]')).not.toBeNull();
  });
});
