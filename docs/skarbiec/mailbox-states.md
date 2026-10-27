# Mailboxes by state

`skrzynka mailbox list --state <STATE>` (HTTP `GET /v1/mailboxes?state=`)
lists only the mailboxes of the organization in one state, after reading
which mailboxes Skarbiec declares, as every `mailbox list` does:

| State | A mailbox is in it when | Skrzynka Desktop |
| --- | --- | --- |
| `failing` | it is declared and its last sync recorded an error (`last_error_code`) | **Mailboxes** facet **Failing** |
| `undeclared` | Skarbiec no longer declares it, so nothing reads it (`enabled: false`) | **Not declared** |
| `unsynchronized` | it has never synchronized (`last_sync_at: null`) | **Never synchronized** |

Each mailbox is printed as `mailbox list` prints it, with its
`recovery_command` and observed `credential_source`, so a script watching
`--state failing` reads what to run to recover it. A state that holds no
mailbox lists `[]`. Desktop's facets count and list the same mailboxes from
the same fields.

## The mailboxes as a table

`skrzynka mailbox list [--state <STATE>] --csv <PATH>` (HTTP `GET
/v1/mailboxes/csv?state=`, `text/csv` offered as `mailboxes.csv`) writes the
mailboxes listed, in the listing's order, to a new file as a CSV table (RFC
4180, CRLF lines) under the header `id, email, display_name, enabled,
imap_host, smtp_host, last_uid, last_sync_at, last_error_code,
last_error_message, created_at`, a value the mailbox does not hold left
empty, and prints `{path, mailboxes, size_bytes}`: the inventory and how each
mailbox's last sync ended, for a spreadsheet or a report. The Skarbiec item
and every credential are never written. Desktop: **Table of mailboxes** in
the Mailboxes inspector while no mailbox is selected, **Mailboxes in** (every
mailbox or one state) and **Export as a table (.csv)…**.

## Refusals

| Refusal | When |
| --- | --- |
| Clap, exit 2 | `--state` names no state above, or is given with `--inbound`; `--csv` is given with `--inbound`. |
| HTTP 400 | `state=` names no state above. |
| `MAILBOX_OUTPUT_EXISTS` | `--csv` names a path that exists; refused before the vault is read. |
| `MAILBOX_OUTPUT_INVALID` | `--csv` names a path that cannot be checked, created or written, with the cause. |

## Real test

`tests/mailbox/states.mjs` runs the built CLI against an organization
holding real mailboxes. It reads `mailbox list` and checks that `--state
failing`, `--state undeclared` and `--state unsynchronized` each list
exactly the mailboxes the full listing shows in that state, and that an
unknown state and `--state` with `--inbound` are refused. It writes `--csv`
to its run directory and checks the table holds the full listing's
mailboxes in its order with their addresses, a failing one's error code,
and no Skarbiec item column, that the same path again is
`MAILBOX_OUTPUT_EXISTS` and that `--csv` with `--inbound` is refused. It
changes nothing else.

```sh
node tests/mailbox/states.mjs --organization <ORG> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/mailbox-states/<run>/report.json`.
