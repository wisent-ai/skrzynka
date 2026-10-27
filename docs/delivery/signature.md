# Mailbox signature

A mailbox can sign what it sends. The signature follows the body after the
RFC 3676 separator line `-- `, so mail programs recognise and fold it.

| Surface | Read | Set | Remove |
| --- | --- | --- | --- |
| CLI | `skrzynka mailbox signature <ID>` | `skrzynka mailbox signature <ID> --file <PATH>` | `skrzynka mailbox signature <ID> --clear` |
| HTTP | `GET /v1/mailboxes/:id/signature` | `PUT /v1/mailboxes/:id/signature` with `{"signature": "…"}` | `PUT` with `{"signature": null}` or `{}` |
| Desktop | **Signature** in the mailbox inspector | **Save signature** | **Remove signature** |

Every form answers `{mailbox_id, signature}`; `signature` is `null` when the
mailbox signs nothing. Setting it needs the organization's admin role.

## What is signed

Replies, originated messages and forwards carry the signature. Unsubscribe
mail does not: it is a request to the list's machine.

Each reply and outbound row records the signature at the moment it is
claimed (`signature` on the row `message replies` and `message outbound`
print), and that is what the send carries. Changing the signature later does
not change a send already claimed, and repeating a request with its
idempotency key answers the first attempt with the first signature. The
`body` field stays your text alone, so the same key with the same text is
still the same request after the signature changed.

## Refusals

| Code | When |
| --- | --- |
| `SIGNATURE_INVALID` | The signature holds no visible text, or `--file` names a file that cannot be read, is not a regular file, or is not UTF-8 (the path and cause are named). |
| `NOT_FOUND` | The organization holds no mailbox with that ID. |

`--file` and `--clear` together are refused by Clap before dispatch.

## Real test

`tests/delivery/signature.mjs` runs the built CLI against a dedicated real
mailbox and a recipient the operator names. It sets a freshly drawn
signature, sends one real message and checks the outbound row records that
signature and its body is the text alone, changes the signature and checks a
repeat with the same key still answers the first row, removes the signature
and checks it reads as absent, and checks an empty signature file is refused.
The mailbox's prior signature is restored at the end.

```sh
node tests/delivery/signature.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-signature/<run>/report.json`.
