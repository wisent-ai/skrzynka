# Sender authentication verdicts

Every import records the receiving provider's `Authentication-Results`
headers (RFC 8601) in the message's `authentication` field: what the provider
concluded when it checked the sender with SPF, DKIM, DMARC and other methods.
Skrzynka checks nothing itself and changes nothing the provider wrote.

`skrzynka message show <ID>`, `skrzynka message list`, `GET /v1/messages` and
`GET /v1/messages/:id` carry the field; Skrzynka Desktop shows it as
**Sender authentication** in the message inspector, with every verdict that
is not `pass` marked.

| Field | Meaning |
| --- | --- |
| `authserv_id` | Who checked, as the header names it (the provider's server). |
| `method` | `spf`, `dkim`, `dmarc`, `arc` or another method, lower case. |
| `result` | `pass`, `fail`, `softfail`, `neutral`, `none`, `temperror`, `permerror`, lower case. |
| `detail` | The rest of the clause: the reason comment and properties, as written. |

A clause that is not `method=result` is kept whole in `detail` with an empty
`method` and `result`. The field is `[]` when the provider wrote no such
header, and `null` for a message imported before verdicts were recorded.

## Two uses

- **Reading mail:** a message that claims a bank's address but whose `dmarc`
  verdict is `fail` was not proven to come from it. `message list
  --failed-authentication` (`failed_authentication=true`; Desktop's
  **Failed authentication** in the filter bar) lists every stored message
  with a verdict other than `pass`, as
  [finding stored mail](../messages/filters.md) describes.
- **Checking your own sending:** send from one mailbox to another mailbox you
  have declared, sync the receiving one, and read the verdicts on the
  received copy. `spf`, `dkim` and `dmarc` all `pass` means the receiving
  provider accepts your domain's setup; any other result names what to fix in
  the domain's DNS.

## Real test

`tests/delivery/authentication.mjs` runs the built CLI against a dedicated
real mailbox whose provider writes `Authentication-Results` (Gmail does). It
syncs the mailbox, requires at least one stored message with verdicts, and
checks every verdict is either `method=result` with both parts or a whole
clause in `detail`, each with the checker named.

```sh
node tests/delivery/authentication.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-authentication/<run>/report.json`.
