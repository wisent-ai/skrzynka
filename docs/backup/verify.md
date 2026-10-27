# Checking an export

`skrzynka message verify --input <DIRECTORY>` (Skrzynka Desktop:
**Check an export…** in the mailbox inspector's Export section) proves a
backup [`message export`](export.md) wrote still holds what was written,
before it is needed:

- `manifest.json` in the directory is read;
- every file it lists under `exported`, and the `mailbox.mbox` it names
  under `mbox` when the export wrote one, is read and its SHA-256 compared
  with the one the export recorded;
- nothing is written and no provider is contacted, so a backup kept on
  another disk or machine can be checked as often as wanted.

The command prints `{directory, verified, damaged}`: every listed file that
still holds what was written, and every one that does not with its code
and sentence. When any is damaged the command ends with `EXPORT_DAMAGED`
(exit 1) after printing, so a script cannot take a damaged backup for a
whole one. Desktop says how many files were checked, or names each damaged
one with why.

[`message restore`](restore.md) checks each original the same way before
appending it and refuses a changed one with `RESTORE_FILE_CHANGED`;
`message verify` checks the whole directory, the mbox included, without
restoring anything.

## Refusals

| Code | When |
| --- | --- |
| `EXPORT_INPUT_INVALID` | `manifest.json` is missing, unreadable or not an export manifest. |
| `EXPORT_FILE_MISSING` | A listed file could not be read, or the manifest names a path outside the directory; listed under `damaged`. |
| `EXPORT_FILE_CHANGED` | A file's SHA-256 differs from the one the export recorded; listed under `damaged`. |
| `EXPORT_DAMAGED` | At least one listed file is damaged; `damaged` names each and why. |

A missing `--input` is refused by Clap with exit 2.

## Real test

`tests/backup/verify.mjs` runs the built CLI against a dedicated real
mailbox. It exports the mailbox with `--mbox` into the run's report folder
and checks `message verify` verifies every listed file, the mbox included,
and reports nothing damaged; then copies the export, changes one original,
removes another and changes the mbox, and checks the copy is refused with
`EXPORT_DAMAGED` listing exactly those three, `EXPORT_FILE_CHANGED`,
`EXPORT_FILE_MISSING` and `EXPORT_FILE_CHANGED`, with every other file
verified; and checks a directory without a manifest is refused with
`EXPORT_INPUT_INVALID`. Nothing is written to the provider.

```sh
node tests/backup/verify.mjs --organization <ORG> --mailbox <ID> \
  --binary <PATH> --source-revision <COMMIT>
```

Reports remain under `.build/real-tests/verify-export/<run>/report.json`.
