# Connecting Gmail

Before reaching for any of the three Gmail paths, ask which one this account
can actually use. Every verdict is measured when you ask: the stored password
is put through a real IMAP login at `imap.gmail.com` on the port
`/etc/services` registers for `imaps`, the stored OAuth client and the
loopback redirect named by `--bind` are handed to Google once and its answer
read back, and a Workspace address gets a real delegated token mint. `--bind`
is the loopback address and port `account authorize` would listen on; no
address is assumed.

```sh
target/debug/skrzynka account connection --provider gmail --bind LOOPBACK:PORT --email user@example.com
```

It reports `app_password`, `oauth` and `delegation`, each `usable`, `refused`
or `unproven`, with the refusal code, what was observed, and the exact next
step. `usable_paths` counts the ones that authenticated. A path is never
`usable` because a Skarbiec item exists: `unproven` is what a readable
declaration earns. Without `--email` only the account-independent state of
each path is reported. The command exits 0 whenever the report was produced,
including when no path works; it exits 1 only when the argument is not an
address or the address names more than one mailbox.

## One account with an app-specific password

Generate an app-specific password in that account, then pass it to Skrzynka
only through stdin:

```bash
read -rsp "Gmail app-specific password: " GMAIL_APP_PASSWORD; printf '\n'
printf '%s\n' "$GMAIL_APP_PASSWORD" |
  target/debug/skrzynka account password --provider gmail --email user@example.com
unset GMAIL_APP_PASSWORD
target/debug/skrzynka sync
target/debug/skrzynka message list
```

When the Gmail login address differs from the address recipients know, the
Skarbiec item says so itself: `username` is the login and `email` is the
public address used as `From`.

The command first logs in to `imap.gmail.com` over TLS, on the port
`/etc/services` registers for `imaps`; a database that cannot be read or
registers no such port refuses with `IMAP_PORT_UNREGISTERED` before any
connection. Only after Google accepts the credential does it write the
deterministic `skrzynka-gmail-app-password-*` bundle to Skarbiec and tag it
`skrzynka:mailbox`; the next reconciliation creates a mailbox using
`smtp.gmail.com` with STARTTLS. If declaring fails after the bundle is saved,
the refusal names the saved Skarbiec item. This is a one-user,
one-app-password path: it needs neither a Workspace administrator nor any
OAuth client. The password never appears in argv, Skrzynka logs, its database,
or its loopback API. A LOGOUT the server does not answer cleanly after the
login is logged as a warning and does not undo the proof.

## OAuth

```sh
target/debug/skrzynka account authorize --provider gmail --bind LOOPBACK:PORT --skarbiec-item <google-login-item>
```

It prints Google's authorization URL and starts serving the loopback callback
while asking Google whether that client accepts the redirect. A completed
callback settles the command without waiting for the diagnostic response.
If the diagnostic answers first with `redirect_uri_mismatch`, the command
refuses with `GMAIL_OAUTH_REDIRECT_NOT_REGISTERED`, naming the client and
redirect URI. Otherwise it waits without a deadline: it exits 0 after the
callback stores a dedicated `skrzynka-gmail-*` OAuth bundle and declares it a
mailbox in Skarbiec, or exits non-zero with `GMAIL_OAUTH_FAILED` carrying the
refusal the callback reported (declined consent, an invalid code, a failed
token exchange). Interrupt it to abandon an authorization nobody completes.
Binding failures name the requested socket and operating-system cause. The CLI
also observes its callback server while awaiting authorization: an early exit,
serve error or failed task is reported as `INTERNAL_ERROR` with the socket and
observed cause, rather than waiting for a callback that cannot be processed.
An authorization-page diagnostic request that fails reports
`GMAIL_OAUTH_DIAGNOSIS_FAILED` with its transport cause or HTTP status, without
printing the authorization URL. The explicit connection probe retains this
failure; during authorization the CLI prints it to stderr and keeps serving
the real callback. A failed diagnosis is not evidence that Google accepted
the redirect, and it does not invalidate a subsequently completed callback.

The redirect is refused when the OAuth client Skarbiec holds for the role
`skrzynka-google-oauth-desktop` has no loopback redirect, so no consent screen
is reachable. A **Desktop app** client, described in the
[Skarbiec mailbox contract](../skarbiec/mailbox-contract.md), avoids that.
Without one, use an app-specific password for one account or domain-wide
delegation for an administrator-managed Workspace domain.
