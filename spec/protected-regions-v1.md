# Protected Mail Regions — Interoperability Specification v1 (Draft)

**Status:** Draft / Proposal
**Scope:** A vendor-neutral, additive convention for marking and protecting sensitive
spans in ordinary email so that **recipients (and their automated/AI processing) do not
inadvertently receive the plaintext**, while remaining a normal RFC 5322 message.

> This spec is the **publishable, vendor-neutral** carve-out of an app's internal design.
> It contains only what independent clients need to interoperate — no app internals.

---

## 1. Goals & non-goals

- **Goal:** Reduce *inadvertent* exposure of private content to recipient-side cloud AI,
  while keeping full compatibility with clients that do not implement this spec.
- **Goal:** Additive only — non-supporting clients simply ignore the extra field and see
  masked text; the message is never broken.
- **Non-goal:** Strong E2E security by itself (see §6 on key levels). The baseline targets
  *accidental* exposure, not a determined adversary.

## 2. Core rule

> **A protected region is shown in full to a human, but handed to any AI/automated
> processing in masked form.** Conforming clients MUST mask protected regions before
> sending content to AI.

## 3. Body markers (masking)

Sensitive spans are replaced in the body by a neutral, plain/HTML-safe marker:

```
/////<field-name>/////
```

- `field-name` is an arbitrary token (e.g. `privacy_address`).
- Any client renders this as plain masked text. Angle-bracket/tag forms MUST NOT be used.

## 4. The `X-Protected-Regions` header

Carries metadata and (small) ciphertext, or references a MIME part. **No plaintext secret.**

```
X-Protected-Regions: v=1; regions=privacy_address,privacy_phone;
    alg=AES-256-GCM; enc=base64; key=<key-id>; data=<base64-ciphertext>
```

| Param | Meaning |
|-------|---------|
| `v`       | Spec version (`1`) |
| `regions` | Comma-separated field-names present |
| `alg`     | Encryption algorithm (`AES-256-GCM`) |
| `enc`     | Encoding of `data` (`base64`) |
| `key`     | **Key identifier** (self-describing; see §5) |
| `data`    | Ciphertext (small payloads) — OR omit and use `part` |
| `part`    | `Content-ID` of a MIME part holding the ciphertext (larger payloads) |

The decrypted payload maps `field-name → plaintext value`.

**Robustness:** Headers may rarely be stripped by mailing lists / gateways. Larger
ciphertext SHOULD live in a MIME part (body parts survive better). The masked body always
survives, so the message degrades gracefully.

## 5. Key identification & rotation (no dates)

- Each message is **self-describing**: it names the key it used via `key=<key-id>`.
- Decryption selects the key **by `key-id` only — never by date/time**.
- Implementations keep a registry `{ key-id → key material }` and **never delete old keys**
  (or old messages become unreadable). Rotation = add a new key, mark it current for
  encryption. Old messages keep their `key-id` and still decrypt.
- Key metadata (created/retired/status) MAY be recorded for operations/audit, but MUST NOT
  drive decryption selection.

## 6. Key levels (progressive)

| Level | Key | Who can decrypt |
|-------|-----|-----------------|
| L1 | Shared application key (`key-id` e.g. `appkey-v1`) | Anyone with that app/key (accident-prevention grade) |
| L2 | Per-user keypair (public-key encryption to recipient) | Intended recipient only |
| L3 | Per-message password / out-of-band | Whoever has the password |
| L4 | S/MIME or OpenPGP | Standard E2E |

Conformance MAY start at L1 and advance; the wire format (§4–§5) is identical across levels.

## 7. Fallback: password-protected PDF (optional)

For recipients that do not implement this spec:

- Attach the real values as a **password-protected PDF (AES-256)**.
- The password MAY be placed in the message body (accident-prevention threat model).
- Note: some gateways block encrypted attachments; the masked body remains the safe baseline.

## 8. Conformance

A conforming client:
1. MUST render `/////name/////` as masked text if it cannot decrypt.
2. MUST mask protected regions before any AI/automated processing (§2).
3. SHOULD decrypt and display the real value to the human when it holds the key.
4. MUST select keys by `key-id` (§5), never by date.
5. MUST treat the field as additive — never break non-supporting messages.

## 9. Versioning

- `v=1`. Future versions add params without breaking v1 parsing. Already-sent messages
  remain valid forever (self-describing `v` and `key`).

## 10. Security considerations

- L1 shared keys are extractable from a client → accident-prevention grade only.
- Header/attachment may be stripped/blocked on some paths → rely on graceful degradation.
- The masked body still reveals that *something* is hidden, plus surrounding context.

---

*This is a draft proposal intended to seed an open convention. Feedback and independent
implementations are welcome.*
