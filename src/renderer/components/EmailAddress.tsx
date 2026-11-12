// メール詳細のアドレス（差出人・宛先・CC・本文中）と、その横の＋/編集ボタン・右クリックメニュー。
//
// ＋/編集の判定（willEdit）は、＋ボタンと右クリックメニューで同じものを使う（useContactAction）。

import { useEffect, useState, type MouseEvent, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { Copy, Mail, Pencil, Plus, UserPlus } from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import { contactLookupEmail } from '../services/contacts';
import { copyText } from '../utils/clipboard';
import { contactNamed } from '../utils/senderContact';
import { ContextMenu, type MenuItem } from './ContextMenu';

/** 「表示名 <メール>」に整形。表示名が無ければアドレスのみ。 */
export function formatAddress(name: string | null, address: string | null): string {
  const a = (address ?? '').trim();
  const n = (name ?? '').trim();
  if (!a) return '—';
  return n ? `${n} <${a}>` : a;
}

/** 本文/ヘッダ中のメールアドレス検出。 */
const EMAIL_RE = /([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g;

/** メールアドレスごとの住所録照合キャッシュ（同一アドレスの重複ルックアップを避ける）。
 *  住所録が変わり得るので、メール画面を離れるとき（MailBody アンマウント）にクリアする。 */
const emailLookupCache = new Map<string, Promise<ContactSummary[]>>();
const lookupKey = (email: string) => email.trim().toLowerCase();
function lookupEmailCached(email: string): Promise<ContactSummary[]> {
  const key = lookupKey(email);
  let p = emailLookupCache.get(key);
  if (!p) {
    p = contactLookupEmail(email).catch(() => [] as ContactSummary[]);
    emailLookupCache.set(key, p);
  }
  return p;
}

/** 照合キャッシュを空にする（メール画面を離れるとき）。 */
export const clearEmailLookupCache = () => emailLookupCache.clear();

/** そのアドレスの照合だけ捨てる（住所録に登録した直後など）。 */
export const forgetEmailLookup = (email: string) => emailLookupCache.delete(lookupKey(email));

/** アドレスに対する住所録の操作（＋で追加 / 編集）。 */
type ContactActionProps = {
  email: string;
  name?: string | null;
  onAdd: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  /** アドレスのクリックでこのアドレス宛の新規メール作成を開く。 */
  onCompose?: (email: string) => void;
};

/**
 * ＋ボタンと右クリックメニューで共有する判定。住所録に未登録＝追加、名前が関連する
 * 登録済みがあれば編集。重複（複数登録）の件数も返す。
 */
function useContactAction({ email, name, onAdd, onEdit }: ContactActionProps) {
  const { t } = useTranslation();
  const [matches, setMatches] = useState<ContactSummary[] | null>(null);
  useEffect(() => {
    let alive = true;
    lookupEmailCached(email).then((r) => alive && setMatches(r));
    return () => {
      alive = false;
    };
  }, [email]);

  const count = matches?.length ?? 0;
  const registered = count > 0;
  const dup = count > 1;

  // 差出人名に「関連する」既存を優先（contactNamed。前任の取り違えを防ぐ）。
  const wantName = (name ?? '').trim();
  const named = matches ? contactNamed(matches, name) : null;
  // クリック動作: 名前が関連する既存があれば編集。差出人名が無ければ（判別できないので）
  // 従来どおり編集。差出人名があるのに関連する既存が無い＝別人とみなして新規追加にする
  // （アドレスが 1 件だけ一致でも、名前が違えば開かず新規登録にする）。
  const willEdit = registered && !!onEdit && (named != null || !wantName);

  const run = () => {
    if (willEdit && matches && onEdit) onEdit((named ?? matches[0]).id);
    else onAdd(name ?? null, email);
  };
  const title = willEdit
    ? dup
      ? t('mailbox.editContactDup', { count })
      : t('mailbox.editContact')
    : t('mailbox.addContact');
  return { run, title, willEdit, dup, count };
}

/**
 * 本文中の日付・アドレスの横に、ホバー/フォーカスで現れる＋ボタン（の外枠）。
 * - 幅 0 で右へはみ出して描く: 見えない間も幅を取らず（「12:50」と「:57」の間に隙間を作らない）、
 *   現れても文字が左右に動かない
 * - inline-flex にしない: flex の中身はブロック扱いになり、選んで写すと前後に改行が入る
 *   （Chromium / WebView2 で確かめた）
 * - **親が overflow:hidden（truncate 等）だと右へはみ出した分が切られる。**ヘッダの差出人の行は
 *   省略するので、こちらではなく幅を取る HOVER_ADD_BUTTON_SIZED を使う
 */
export const HOVER_ADD_BUTTON =
  'inline-block w-0 overflow-visible whitespace-nowrap align-middle opacity-0 transition-opacity focus:opacity-100 focus:outline-none';

/**
 * 幅を取る＋ボタン（の外枠）。省略される範囲の外に置き、切られずに確かな大きさで押せる
 * （ヘッダの差出人・宛先・CC）。見えない間も幅は取るが、置くのは行の中の区切り（アドレスの後ろ）だけ。
 * 中身は svg だけなので、選んで写しても文字は混ざらない。
 */
const HOVER_ADD_BUTTON_SIZED =
  'inline-block shrink-0 whitespace-nowrap align-middle opacity-0 transition-opacity focus:opacity-100 focus:outline-none';

/**
 * ＋ボタンの見た目（丸い枠）。後ろの文字に重なって出るので、背景は不透明にして手前に描く。
 * 中身も inline-flex にしない（上と同じ理由）。
 */
export const HOVER_ADD_PILL =
  'relative z-10 ml-0.5 inline-block h-4 rounded-full bg-zinc-700 px-1 leading-4 text-white/80 shadow hover:bg-sky-600 hover:text-white [&>svg]:inline [&>svg]:align-[-1px]';

/** ＋/編集ボタン本体（重複件数つき）。`sized` なら幅を取る（切られる親の外に置くとき）。 */
function ContactActionButton({
  action,
  sized,
  groupHover,
}: {
  action: ReturnType<typeof useContactAction>;
  sized?: boolean;
  /** ホバーで出すきっかけの group 名に合わせた class（Tailwind は字面で拾うので丸ごと渡す）。 */
  groupHover: string;
}) {
  const { run, title, willEdit, dup, count } = action;
  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        run();
      }}
      title={title}
      aria-label={title}
      className={`${sized ? HOVER_ADD_BUTTON_SIZED : HOVER_ADD_BUTTON} ${groupHover}`}
    >
      {/* 重複件数は ::after で出す（文字にしないので、写したときに件数が混ざらない）。 */}
      <span
        data-count={dup ? count : undefined}
        className={`${HOVER_ADD_PILL} ${
          dup
            ? 'after:ml-0.5 after:text-[9px] after:font-semibold after:text-amber-300 after:content-[attr(data-count)]'
            : ''
        }`}
      >
        {willEdit ? <Pencil size={11} /> : <Plus size={11} />}
      </span>
    </button>
  );
}

/** アドレスの右クリックメニュー（住所録に追加/編集・アドレスをコピー・このアドレスへ新規メール）。 */
function useAddressMenu(
  email: string,
  action: ReturnType<typeof useContactAction>,
  onCompose?: (email: string) => void
) {
  const { t } = useTranslation();
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const items: MenuItem[] = [
    {
      key: 'contact',
      label: action.title,
      Icon: action.willEdit ? Pencil : UserPlus,
      onClick: action.run,
    },
    {
      key: 'copy',
      label: t('compose.recipientCopyAddress'),
      Icon: Copy,
      onClick: () => void copyText(email).catch(() => undefined),
    },
    ...(onCompose
      ? [
          {
            key: 'compose',
            label: t('mailbox.composeTo', { email }),
            Icon: Mail,
            onClick: () => onCompose(email),
          },
        ]
      : []),
  ];
  const onContextMenu = (e: MouseEvent) => {
    // 文字を選んでいるときは、選択のメニュー（コピー等。親の右クリック）に任せる。
    if ((window.getSelection()?.toString() ?? '').trim().length > 0) return;
    e.preventDefault();
    e.stopPropagation();
    setMenu({ x: e.clientX, y: e.clientY });
  };
  const node = menu ? (
    <ContextMenu x={menu.x} y={menu.y} header={email} items={items} onClose={() => setMenu(null)} />
  ) : null;
  return { onContextMenu, node };
}

/** アドレスの文字（クリックで新規メール。無ければ素の文字）。ボタンは既定で中央寄せなので、
 *  狭い幅で折り返したときに字下げのように見えないよう左寄せにする。 */
function AddressText({
  email,
  onCompose,
  onContextMenu,
}: {
  email: string;
  onCompose?: (email: string) => void;
  onContextMenu: (e: MouseEvent) => void;
}) {
  const { t } = useTranslation();
  return onCompose ? (
    <button
      type="button"
      onClick={() => onCompose(email)}
      onContextMenu={onContextMenu}
      title={t('mailbox.composeTo', { email })}
      className="cursor-pointer rounded text-left hover:text-sky-300 hover:underline focus:outline-none focus:text-sky-300"
    >
      {email}
    </button>
  ) : (
    <span onContextMenu={onContextMenu}>{email}</span>
  );
}

/**
 * メールアドレス＋（ホバー/フォーカスで現れる）操作ボタン。本文中・宛先の並びで使う。
 * 住所録に未登録＝＋（追加）、登録済み＝編集アイコン。重複（複数登録）があれば件数を黄色字で表示。
 * 右クリックで同じ操作とコピー・新規メールのメニューを出す。
 */
export function EmailAdd(props: ContactActionProps) {
  const action = useContactAction(props);
  const menu = useAddressMenu(props.email, action, props.onCompose);
  return (
    <span className="group/email">
      <AddressText
        email={props.email}
        onCompose={props.onCompose}
        onContextMenu={menu.onContextMenu}
      />
      <ContactActionButton action={action} groupHover="group-hover/email:opacity-100" />
      {menu.node}
    </span>
  );
}

/**
 * ヘッダの差出人。「表示名 <アドレス>」は長いと省略（truncate）し、＋/編集ボタンは**省略される
 * 範囲の外**（行の右）に幅を取って置く。幅 0 ではみ出して描くと、truncate の overflow:hidden で
 * 丸が切れて「>」付近に欠けた丸しか見えなかった（vaio・WebView2・alpha.67 の報告）。
 *
 * - `prefix`: 「差出人:」などの見出しとバッジ（省略される側に入れる）
 */
export function SenderAddressLine({
  name,
  address,
  prefix,
  onAdd,
  onEdit,
  onCompose,
}: {
  name: string | null;
  address: string | null;
  prefix: ReactNode;
  onAdd?: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  onCompose?: (email: string) => void;
}) {
  const a = (address ?? '').trim();
  if (!a || !onAdd) {
    return (
      <span className="min-w-0 truncate">
        {prefix}
        {formatAddress(name, address)}
      </span>
    );
  }
  return (
    <SenderWithAction
      name={name}
      email={a}
      prefix={prefix}
      onAdd={onAdd}
      onEdit={onEdit}
      onCompose={onCompose}
    />
  );
}

function SenderWithAction({
  name,
  email,
  prefix,
  onAdd,
  onEdit,
  onCompose,
}: {
  name: string | null;
  email: string;
  prefix: ReactNode;
  onAdd: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  onCompose?: (email: string) => void;
}) {
  const n = (name ?? '').trim();
  const action = useContactAction({ email, name: n || null, onAdd, onEdit });
  const menu = useAddressMenu(email, action, onCompose);
  return (
    // 外枠は flex（省略する文字の塊と、ボタンの 2 つだけを並べる）。文字の塊の中は inline のままなので、
    // 選んで写しても名前・アドレス・「>」の間に改行は入らない（HOVER_ADD_BUTTON の注意）。
    <span className="group/email flex min-w-0 items-baseline">
      <span className="min-w-0 truncate">
        {prefix}
        {n && <>{n} &lt;</>}
        <AddressText email={email} onCompose={onCompose} onContextMenu={menu.onContextMenu} />
        {n && <>&gt;</>}
      </span>
      <ContactActionButton action={action} sized groupHover="group-hover/email:opacity-100" />
      {menu.node}
    </span>
  );
}

/**
 * 宛先・CC・Reply-To などアドレスの並び（文字列）の中のアドレスに＋/編集を出す。
 * 行は折り返す（break-words）ので overflow で切られない。＋は幅 0 で後ろの「, 」に重ねて出す。
 */
export function LinkifyEmails({
  text,
  onAdd,
  onEdit,
  onCompose,
}: {
  text: string;
  onAdd?: (name: string | null, email: string) => void;
  onEdit?: (id: number) => void;
  onCompose?: (email: string) => void;
}) {
  if (!onAdd) return <>{text}</>;
  const parts = text.split(EMAIL_RE);
  return (
    <>
      {parts.map((p, i) =>
        i % 2 === 1 ? (
          <EmailAdd key={i} email={p} onAdd={onAdd} onEdit={onEdit} onCompose={onCompose} />
        ) : (
          <span key={i}>{p}</span>
        )
      )}
    </>
  );
}
