import { useEffect, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { LeafyGreen } from 'lucide-react';
import type { ContactSummary } from '@bindings/ContactSummary';
import { greenDomainAdd, greenDomainWarn } from '../services/green';
import { findKeptContact, isDomainGreen } from '../utils/senderContact';
import {
  FreemailGreenPrompt,
  GREEN_NOTICE_BOX,
  GREEN_NOTICE_PRIMARY,
  GREEN_NOTICE_SECONDARY,
  GREEN_NOTICE_TEXT,
} from './FreemailGreenPrompt';

/** 葉っぱを押したあとに出すもの。 */
type Panel =
  /** フリーメールで断られた（住所録に登録して「この人だけ」グリーンにするかを訊く）。 */
  | { kind: 'freemail' }
  /** 外そうとしたが、住所録の本人なのでグリーンのまま。`domainUnset` ならドメインの認定は外した。 */
  | { kind: 'kept'; contact: ContactSummary; domainUnset: boolean };

/**
 * 差出人のグリーン（葉っぱ）の付け外し。メール詳細（MailBody）と会話のバブル（Conversation）で共有する
 * （docs/GREEN_DOMAINS.md）。
 *
 * グリーンは「ドメインがグリーン」か「住所録の本人」のどちらか。葉っぱはドメインの認定を付け外しするので、
 * 住所録の本人は葉っぱでは外せない（住所録を勝手に消したり書き換えたりしない）。外そうとしたら:
 * - 本人でなければ: ドメインを警告に回し、グリーンを外す（従来どおり）
 * - 本人で、ドメインもグリーン: ドメインだけ警告に回し、本人の分でグリーンのままであることを伝える
 * - 本人で、ドメインはグリーンでない（フリーメール等）: 何も書かず、外せない理由と「住所録で開く」を出す
 * 見た目は結果どおりにだけ変える（外したつもりで一瞬消えて、読み直すと戻る、をしない）。
 */
export function useSenderGreen({
  domain,
  address,
  name,
  baseGreen,
  resetKey,
  onChanged,
  onOpenContact,
}: {
  /** 差出人のドメイン（自分のメール等で無ければ空）。 */
  domain: string;
  address: string;
  name: string | null;
  /** 読み込んだ時点のグリーン（is_green）。 */
  baseGreen: boolean;
  /** これが変わったら（別のメールを開いたら）付け外しの途中の状態を捨てる。 */
  resetKey: unknown;
  /** グリーンを変えた・ドメインの認定を変えた（新しい見た目の値）。親は一覧を読み直す。 */
  onChanged?: (green: boolean) => void;
  /** 住所録でその人を開く（無ければ「住所録で開く」を出さない）。 */
  onOpenContact?: (id: number) => void;
}): {
  isGreen: boolean;
  toggle: () => Promise<void>;
  setGreen: (g: boolean) => void;
  panel: ReactNode;
} {
  const [override, setOverride] = useState<boolean | null>(null);
  useEffect(() => setOverride(null), [resetKey, baseGreen]);
  const [panel, setPanel] = useState<Panel | null>(null);
  useEffect(() => setPanel(null), [resetKey]);
  const isGreen = override ?? baseGreen;

  const toggle = async () => {
    if (!domain) return;
    setPanel(null);
    try {
      if (!isGreen) {
        if (!(await greenDomainAdd(domain))) {
          setPanel({ kind: 'freemail' });
          return;
        }
        setOverride(true);
        onChanged?.(true);
        return;
      }
      // 外す: まず住所録の本人かを引き直す（読み込んだ時点の is_known は古いことがある）。
      const kept = await findKeptContact(address, name);
      if (!kept) {
        await greenDomainWarn(domain);
        setOverride(false);
        onChanged?.(false);
        return;
      }
      const domainUnset = await isDomainGreen(domain);
      if (domainUnset) {
        await greenDomainWarn(domain);
        onChanged?.(true);
      }
      setPanel({ kind: 'kept', contact: kept, domainUnset });
    } catch {
      /* noop */
    }
  };

  const close = () => setPanel(null);
  let node: ReactNode = null;
  if (panel?.kind === 'freemail' && domain) {
    node = (
      <FreemailGreenPrompt
        domain={domain}
        name={name}
        address={address}
        onGreen={() => {
          setOverride(true);
          onChanged?.(true);
        }}
        onClose={close}
      />
    );
  } else if (panel?.kind === 'kept') {
    node = (
      <GreenKeptNotice
        domain={domain}
        address={address}
        contact={panel.contact}
        domainUnset={panel.domainUnset}
        onOpenContact={onOpenContact}
        onClose={close}
      />
    );
  }
  return { isGreen, toggle, setGreen: setOverride, panel: node };
}

/** 住所録の本人なので葉っぱでは外せない、を伝える（ドメインの認定を外したかどうかも）。 */
export function GreenKeptNotice({
  domain,
  address,
  contact,
  domainUnset,
  onOpenContact,
  onClose,
}: {
  domain: string;
  address: string;
  contact: ContactSummary;
  domainUnset: boolean;
  onOpenContact?: (id: number) => void;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const vars = { domain, address, name: contact.display_name };
  return (
    <div role="status" data-testid="green-kept-notice" className={GREEN_NOTICE_BOX}>
      <LeafyGreen size={14} className="mt-0.5 shrink-0 text-emerald-300" aria-hidden />
      <p className={GREEN_NOTICE_TEXT}>
        {domainUnset ? t('green.keptDomainUnset', vars) : t('green.keptPerson', vars)}
      </p>
      <div className="ml-auto flex shrink-0 items-center gap-2">
        {onOpenContact && (
          <button
            onClick={() => {
              onOpenContact(contact.id);
              onClose();
            }}
            className={GREEN_NOTICE_PRIMARY}
          >
            {t('green.openContact')}
          </button>
        )}
        <button onClick={onClose} className={GREEN_NOTICE_SECONDARY}>
          {t('green.freemailClose')}
        </button>
      </div>
    </div>
  );
}
