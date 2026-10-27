# Tag rules

A tag rule gives mail one of your tags as it arrives. Each rule names a tag
and at least one condition; a message gets the tag when it passes every
condition the rule names:

| Condition | The message must… |
| --- | --- |
| `sender` | have a From header containing the text, case aside |
| `list` | have a `List-Id` header containing the text, case aside |
| `contains` | have a subject or stored text containing the text, case aside |
| `recipient` | have a To or Cc header containing the text, case aside: mail sent to one address or alias, or to a domain |
| `attachment` | carry a file whose recorded name contains the text, case aside: invoices, contracts, `.pdf` |

| Verb | CLI | HTTP |
| --- | --- | --- |
| Add | `skrzynka message rule add --tag <TAG> [--sender <TEXT>] [--list <TEXT>] [--contains <TEXT>] [--recipient <TEXT>] [--attachment <TEXT>] [--mailbox <ID or address>]` | `POST /v1/tag-rules` with `{tag, sender, list, contains, recipient, attachment, mailbox}` |
| List | `skrzynka message rule list [--contains <TEXT>] [--paused \| --active] [--mailbox <ID or address>]` | `GET /v1/tag-rules[?contains=&paused=true\|false&mailbox_id=]` |
| Show | `skrzynka message rule show <ID>` (an ID the organization does not hold is `NOT_FOUND`) | `GET /v1/tag-rules/:id` |
| Matches | `skrzynka message rule matches <ID>`: the stored messages the rule matches now, newest first, blocked senders' mail included, paused or not; nothing is tagged (`NOT_FOUND` for an unknown rule) | `GET /v1/tag-rules/:id/matches` |
| Copy | `skrzynka message rule copy <ID> --tag <TAG> [--mailbox <ID or address>]` | `POST /v1/tag-rules/:id/copy` with `{tag, mailbox}` (`mailbox` optional) |
| Update | `skrzynka message rule update <ID> --tag <TAG> [the conditions and --mailbox, as add]` | `PUT /v1/tag-rules/:id` with the same body as add |
| Pause, resume | `skrzynka message rule pause <ID>`, `message rule resume <ID>` | `POST /v1/tag-rules/:id/pause`, `DELETE /v1/tag-rules/:id/pause` |
| Remove | `skrzynka message rule remove <ID>` | `DELETE /v1/tag-rules/:id` (204) |
| Apply | `skrzynka message rule apply [--rule <ID>]` | `POST /v1/tag-rules/apply`, or `POST /v1/tag-rules/:id/apply` for one rule |
| Export | `skrzynka message rule export --output <FILE> [the list flags]` | `GET /v1/tag-rules/export[?` the list parameters `]`, offered as `tag-rules.json` |
| Import | `skrzynka message rule import --input <FILE>` | `POST /v1/tag-rules/import` with `{text}` |

Skrzynka Desktop shows **Tag rules** in the inbox inspector while no message
is selected: each rule with **Matches** (a sheet listing the stored mail
it matches now, each with **Open message**; nothing is tagged), **Apply** (by that rule alone), **Edit**,
**Pause** or **Resume** and **Remove**, a paused rule saying since when,
fields to **Add rule** (among them **To or Cc contains** and **A
file name contains**) with a **Mailbox** choice (every mailbox, or one),
**Save rule** and **Cancel edit** while a rule is being edited, and **Apply
to stored mail**.

A rule is `{id, tag, sender, list, contains, recipient, attachment, mailbox_id, created_at, paused_at}`; the tag and the
conditions are stored trimmed and in lower case. A message that states no To
or Cc header never passes a `recipient` condition, and one carrying no file
with a recorded name, or imported before its files were recorded, never
passes an `attachment` condition.

## One mailbox's mail

`--mailbox` (HTTP `mailbox`, the mailbox's ID or address) keeps a rule to
that mailbox: it tags only mail that mailbox receives, on import and on
`rule apply`, and carries the mailbox as `mailbox_id`. A rule without it
tags mail of every mailbox of the organization (`mailbox_id: null`).
Removing the mailbox's local mail (`mailbox remove`) removes the rules kept
to it.

## Changing a rule

`rule update <ID>` (HTTP `PUT /v1/tag-rules/:id`; Desktop: **Edit**, then
**Save rule**) replaces the rule's tag, every condition and its mailbox with
what is given, checked as `rule add` checks a new rule: a condition left out
is no longer a condition, and a rule left without one is refused. The rule
keeps its `id` and `created_at`, so it keeps its place in the list. Mail
already tagged keeps its tags; every later import, and `rule apply`, tags by
the new conditions. Desktop's **Edit** fills the fields from the rule as
the core holds it when pressed (`GET /v1/tag-rules/:id`, the same as `rule
show <ID>`), so a rule changed or removed from the CLI meanwhile is not
saved over from a stale list.

`rule copy <ID> --tag <TAG>` (Desktop: a tag typed in **Tag**, then
**Copy** beside a rule) makes a new rule with the rule's conditions and the
new tag, kept to the mailbox `--mailbox` names (Desktop: the one chosen
under **Mailbox**) or to the rule's own mailbox, or to every mailbox as the
rule is. It is checked as `rule add` checks a new rule, answers the new
rule, and leaves the rule copied as it was; nothing is tagged until an
import or `rule apply`. An unknown rule is `NOT_FOUND`.

## Pausing a rule

`rule pause <ID>` stops a rule tagging without losing it: it records
`paused_at`, and from then on imports and `rule apply` without `--rule`
leave it out, while the tags it gave stay. Pausing a paused rule keeps when
it was first paused. `rule resume <ID>` clears `paused_at`, so later imports
tag by it again; `rule apply --rule <ID>`, which applies the rule it names
paused or not, tags the mail that arrived meanwhile. Both answer the rule;
a rule the organization does not hold is `NOT_FOUND`. `rule list --paused`
(`paused=true`) lists only the paused rules and `--active` (`paused=false`)
only those tagging now; `--contains <TEXT>` (`contains=`) only those whose
tag or a condition holds the text, case aside, a blank one being
`MESSAGE_FILTER_INVALID`; `--mailbox <ID or address>` (`mailbox_id=`,
Desktop's **Tagging mail of**) only the rules that tag that mailbox's mail:
those kept to it and those of every mailbox, not those kept to another. An
unknown `--mailbox` is `NOT_FOUND`.

## Carrying rules elsewhere

`rule export --output <FILE>` (Desktop: **Export rules…**) writes the rules
`rule list` keeps for the same `--contains`, `--paused`/`--active` and
`--mailbox` to a new file as JSON `[{tag, sender, list, contains,
recipient, attachment, mailbox}]`, oldest first, and prints `{path, rules,
size_bytes}`; `mailbox` is the address of the mailbox a rule is kept to,
which another installation resolves, or `null` for a rule of every
mailbox. A rule's ID and paused state are not carried. An existing path is
`TAG_RULE_OUTPUT_EXISTS` before anything is read. `rule import --input
<FILE>` (Desktop: **Import rules…**) adds every rule of such a file as a
new rule that tags imported mail and prints `{rules}` in file order. The
whole file is read and every rule checked as `rule add` checks one, its
mailbox resolved by ID or address, before any is added: a file that is not
such a list or holds none is `TAG_RULE_INVALID`, and a rule `rule add`
refuses refuses the file with that refusal's code, naming the rule's tag;
nothing is added. Nothing is tagged until an import or `rule apply`.

## When rules tag

Every import (a sync, `serve` reading new mail, a mailbox declared for the
first time) tags each message it writes by every rule of the organization,
in the same transaction as the message. A rule added later does not reach
mail already stored until `rule apply`, which tags every stored message of
the organization, blocked senders' mail included, and answers
`{rules: [{rule_id, tag, messages}]}`, how many messages each rule matched.
`rule apply --rule <ID>` tags by that one rule alone and answers it alone,
so a rule just added reaches the stored mail without the others running.
Tags given are ordinary tags: `message tag --remove` takes one off, and
removing a rule leaves the tags it gave.

## Refusals

| Code | When |
| --- | --- |
| `TAG_RULE_INVALID` | A condition is empty, or the rule names none; for `rule import`, also a file that is not a list of rules or holds none (`…; nothing was imported`). |
| `TAG_RULE_OUTPUT_EXISTS`, `TAG_RULE_OUTPUT_INVALID` | `rule export --output` names a path that exists, or one that cannot be checked, created or written, with the cause. |
| `MESSAGE_TAG_INVALID` | The tag is empty or holds a comma or a control character. |
| `NOT_FOUND` | `rule update`, `rule pause`, `rule resume`, `rule remove` or `rule apply --rule` names a rule the organization does not hold, or `--mailbox` a mailbox it does not hold. |
| `MAILBOX_SELECTOR_AMBIGUOUS` | The `--mailbox` address names more than one mailbox; name it by ID. |

## Real test

`tests/lists/rules.mjs` runs the built CLI against a dedicated real mailbox
holding stored mail. It adds a rule matching the newest message's sender with
a freshly drawn tag, applies the rules and checks every stored message from
that sender, and no other, carries the tag and that the count matches,
removes the rule and checks it is gone while the tags stay, removes those
tags again, and checks the refusals for a rule with no condition and an
empty condition. A second rule for the same sender kept to the mailbox
with `--mailbox` must name it as `mailbox_id`; `rule matches` of it must
list exactly that mailbox's mail from the sender and leave it untagged
(an unknown rule `NOT_FOUND`), and applying it must tag
exactly that mailbox's mail from the sender and count it, `rule list
--mailbox` with the mailbox must list exactly the rules kept to it or to
every mailbox and, with another mailbox, not that rule; a rule kept to an
unknown mailbox, or `rule list --mailbox` with one, is `NOT_FOUND`; `rule
copy` of that rule with a fresh tag must answer a new rule with the same
conditions and mailbox and leave the rule copied as `rule show` read it,
while copying an unknown rule is `NOT_FOUND` and a blank tag
`MESSAGE_TAG_INVALID`. `rule export --contains` that rule's tag must write
exactly its tag, conditions and the mailbox's address, exporting to the
same path is `TAG_RULE_OUTPUT_EXISTS`, `rule import` of the file must add
a new rule with the same tag, conditions and mailbox, and importing an
empty list or a rule with no condition is `TAG_RULE_INVALID` and adds no
rule. A third rule with `--recipient` set to the
mailbox's own address must record it and, applied alone with `--rule`
(answering that rule only), tag exactly the stored messages whose To or Cc
header contains it; an empty `--recipient` is `TAG_RULE_INVALID` and
`apply --rule` with an unknown ID is `NOT_FOUND`. When the stored mail
carries a named file, a fourth rule with `--attachment` set to that name in
upper case must record it in lower case and, applied alone, tag exactly the
stored messages carrying a file whose name contains it; an empty
`--attachment` is `TAG_RULE_INVALID`. The first rule is then updated with
`rule update` to a new tag and a `--contains` condition: it must keep its ID
and `created_at`, read back with only the new tag and condition (the
`--sender` condition gone), be read back the same by `rule show`, and an
update leaving no condition is `TAG_RULE_INVALID` while one naming an
unknown rule, like `rule show` of one, is `NOT_FOUND`. That
rule is then paused: it must read back unchanged but for `paused_at`, be
left out of `rule apply` yet applied by `rule apply --rule`, keep its
`paused_at` when paused again, be listed by `rule list --paused` and not
`--active` while paused, be listed alone by `--contains` its tag in upper
case (a blank `--contains` refused), and read back exactly as before once
resumed; pausing an unknown rule is `NOT_FOUND`.

```sh
node tests/lists/rules.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/tag-rules/<run>/report.json`.
