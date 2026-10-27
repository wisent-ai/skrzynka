# Templates

A template is a text kept under a name to start replies and new messages
from, with the subject a new message from it carries when it names one.
Templates belong to the organization and live only in Skrzynka.

| Surface | Save | List | Read | Rename / copy | Delete |
| --- | --- | --- | --- | --- | --- |
| CLI | `message template save <NAME> --file <PATH> [--subject <TEXT>]` | `message template list [--contains <TEXT>] [--placeholder <KEY>]` | `message template show <NAME>` | `message template rename <NAME> --to <NEW>`, `message template copy <NAME> --to <NEW>` | `message template delete <NAME>` |
| HTTP | `PUT /v1/templates/:name` with `{body, subject}` (`subject` optional) | `GET /v1/templates[?contains=&placeholder=]` | `GET /v1/templates/:name` | `POST /v1/templates/:name/rename` or `/copy` with `{to}` | `DELETE /v1/templates/:name` |
| Desktop | **Templates** → **Subject (optional)**, **Save template** | **Templates** (inbox inspector, no message selected), **Name, subject or text contains** and **Uses placeholder** with **Find** | **Edit** | **Rename** or **Copy**, to the name typed in **Template name** | **Delete** |

Each template is `{name, subject, body, updated_at, placeholders}`, `subject`
`null` for a template saved without one. Saving under a name that exists
replaces its subject and text. A name is trimmed and must hold visible text,
without `/` (it travels in a URL path) or a control character; a subject is
stored trimmed and must be one line of visible text. Renaming
keeps the subject and text; renaming onto a name another template carries is refused
with `TEMPLATE_EXISTS` and changes nothing. Copying keeps them under the
new name too and leaves the original as it was, so one can be changed
without the other; a copy onto a name a template carries, its own
included, is `TEMPLATE_EXISTS`. `list --contains <TEXT>` keeps the
templates whose name, subject or text contains the text, case aside, in name order;
a blank text is `MESSAGE_FILTER_INVALID`. `list --placeholder <KEY>` keeps
the templates whose subject or text names that `{{KEY}}` placeholder, case
aside, so the templates a value is needed for are found before a send; a
blank key is `MESSAGE_FILTER_INVALID`. `export` takes both filters too.

## Carrying templates elsewhere

`message template export --output <FILE> [--contains <TEXT>]` (HTTP `GET
/v1/template-document[?contains=]`, offered as `templates.json`; Desktop:
**Export templates…**) writes every template, or those `--contains` keeps,
to a new file as JSON `[{name, subject, body}]` in name order, and prints
`{path, templates, size_bytes}`; an existing path is
`TEMPLATE_OUTPUT_EXISTS` before anything is read. `message template import
--input <FILE>` (HTTP `POST /v1/template-document` with `{text}`; Desktop:
**Import templates…**) keeps every template such a file holds, replacing one
of the same name as `save` does, and prints `{templates}` in file order.
The whole file is read and every template checked as `save` checks one
before any is kept: a file that is not such a list, holds none, names one
template twice or holds one `save` refuses is `TEMPLATE_INVALID` and
nothing changes.

## Placeholders

A template's subject and text may hold placeholders: `{{key}}`, the key letters,
digits, `_` and `-`, spaces inside the braces aside (`{{ person }}`).
`placeholders` lists each key once, in order of first appearance, the
subject's first. Braces around anything else (`{{not a key}}`) are text and
stay as written.

`skrzynka message template render <NAME> --set KEY=VALUE …` (HTTP `POST
/v1/templates/:name/render` with `{"values": [{"key", "value"}]}`) answers
the template with every placeholder of its subject and text replaced by the
value given for its key; nothing is stored. Every send that takes a template — `reply`, `send`,
`forward`, `draft save|update` with `--template` — takes `--set KEY=VALUE`
the same way, so the text sent is the filled text. A key without a value is
refused with `TEMPLATE_VALUE_MISSING` and a value for a key neither the
subject nor the text names with `TEMPLATE_VALUE_UNUSED`, so a misspelt key never leaves a
placeholder in a sent message. In Skrzynka Desktop, choosing a template with
placeholders from a **Templates** menu shows a field per key and **Fill**
puts the filled text in the body. `draft save --template <NAME> --merge
<TABLE.csv>` fills the template once per row of a CSV table, each row's
columns giving the values and its `to` column the address, and keeps one
draft per row, with the same two refusals naming the row (see
`docs/outreach/merge.md`).

A reply or forward fills three placeholders from the message it answers:
`{{sender}}` with its From address, `{{sender_name}}` with the name the
From header writes, and `{{subject}}` with its subject, each only where the
template names it, the message states it and no `--set` gives it, so a
given value wins. `message reply <ID> --template` and `message forward <ID>
--template` do this for the message `<ID>`; `template render <NAME>
--message <ID>` (HTTP `"message_id"` beside `values`) previews it, an
unknown message being `NOT_FOUND`. Desktop's **Templates** menu beside the
reply composer and Forward sends the message answered, so the fields of
these placeholders may be left blank.

## Using one

`skrzynka message reply <ID> --template <NAME> --idempotency-key <KEY>`,
`skrzynka message send … --template <NAME> …`, `skrzynka message forward
<ID> --to <ADDRESS> --template <NAME> …` and `skrzynka message draft
save|update … --template <NAME>` take the template's text as the body, in
place of `--body-file`; at most one of the two is given. `send` and `draft
save|update` without `--subject` take the template's filled subject; a
`--subject` given wins. A `send` whose template names no subject, given no
`--subject`, is refused with `OUTBOUND_SUBJECT_INVALID` before anything is
claimed. A reply and a forward keep their own `Re:` and `Fwd:` subjects. In
Skrzynka Desktop, the **Templates** menu beside the reply composer's body,
Compose's message, Forward's text and the draft editor puts a template's
text there, replacing what was written, to be edited before sending; on
Compose and in the draft editor it also puts the template's subject in a
**Subject** still blank.

The text is read when the command runs: the send records the body it was
claimed with, so changing or deleting the template later changes nothing
already sent, and a repeated idempotency key after the template changed is
refused with `IDEMPOTENCY_KEY_REUSED` like any other changed body. The send is
held to every rule a send is: the mailbox's send limit and signature apply.

## Refusals

| Code | When |
| --- | --- |
| `TEMPLATE_INVALID` | The name is empty, holds `/` or a control character, the text holds no visible text, or `--subject` holds no visible text or a line break; for `import`, also a file that is not a list of templates, holds none or names one twice (`…; nothing was imported`). |
| `TEMPLATE_OUTPUT_EXISTS`, `TEMPLATE_OUTPUT_INVALID` | `export --output` names a path that exists, or one that cannot be checked, created or written, with the cause. |
| `OUTBOUND_SUBJECT_INVALID` | `message send --template` without `--subject`, and the template names no subject. Nothing is claimed. |
| `TEMPLATE_NOT_FOUND` | No template has the name; the refusal lists the ones there are. |
| `TEMPLATE_EXISTS` | A rename or copy names a name another template carries (a copy, its own too). |
| `TEMPLATE_FILE_INVALID` | `--file` cannot be read as a regular UTF-8 file. |
| `TEMPLATE_VALUE_MISSING` | A placeholder of the template has no value. |
| `TEMPLATE_VALUE_UNUSED` | A value names a key no placeholder of the template names. |
| `TEMPLATE_VALUE_INVALID` | A `--set` is not `KEY=VALUE`, or a key is given twice. |
| `MESSAGE_FILTER_INVALID` | `list --contains` was given a blank text. |

## Real test

`tests/writing/templates.mjs` runs the built CLI against a dedicated real
mailbox and a recipient that accepts its mail. It saves a template under a
fresh name with a subject written with spaces around it and checks the
subject is kept trimmed, checks `list` and `show` answer it, that `list --contains` finds
it by its name in upper case, keeps only templates holding the text, keeps
none for a text none holds and refuses a blank one, checks a blank name, a
blank text and a blank subject are refused with `TEMPLATE_INVALID`, sends a
message with `--template` and no `--subject` and checks the stored outbound
row's subject is the template's and its body the template's
text, renames the template and checks the text stayed and the old name is
`TEMPLATE_NOT_FOUND`, checks a rename onto another template's name is
`TEMPLATE_EXISTS`, copies it and checks the copy holds the subject and text and the
original is unchanged, that a copy onto its own name is `TEMPLATE_EXISTS`
and a copy of an unknown template `TEMPLATE_NOT_FOUND`, deletes the template and checks `show` and `--template`
are then refused with `TEMPLATE_NOT_FOUND`. It saves a template with
placeholders in its subject and text, checks they are listed once each in
order, the subject's first, that `list --placeholder` its subject's key in
upper case lists that template alone among the run's, that a key none names
lists none and a blank one is refused, that `render` fills both (text in braces that is
no key kept), that a missing value, a value for no placeholder and a
malformed `--set` are refused, that `render --message` of a stored message
fills a `{{sender}}` with its From address while a `--set sender=` wins and
an unknown message is `NOT_FOUND`, that a draft started with `--template --set`
holds the filled subject and text and one given `--subject` keeps it, and
that `send --template` of a template saved without a subject, given no
`--subject`, is `OUTBOUND_SUBJECT_INVALID`; that `export --contains` that
template writes exactly `[{name, subject: null, body}]`, that exporting to
the same path is `TEMPLATE_OUTPUT_EXISTS`, that `import` of the file after
the template was deleted gives it back as it was, and that a file naming
one template twice is `TEMPLATE_INVALID` and keeps nothing. A failed run deletes the
templates and the draft it saved.

```sh
node tests/writing/templates.mjs --organization <ORG> --mailbox <ID> \
  --recipient <ADDRESS> --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/templates/<run>/report.json`.
