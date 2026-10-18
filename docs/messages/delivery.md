# Delivery outcome persistence

Replies and outbound messages claim an idempotency-keyed row before SMTP.
After the row reaches `sending`, a source-message, mailbox or credential read
failure attempts to record `failed`; no SMTP submission occurs on those paths.
A provider refusal also attempts to record `failed`. Lost SMTP evidence or a
stopped send worker attempts to record `uncertain`.

If the terminal update cannot be confirmed, the live error retains the original
cause and the database cause. It names `sending` as the last confirmed state,
not the current state: the database update can succeed before its readback fails.
Inspect the stored outcome and provider evidence before creating another attempt.
Repeating the same idempotency key does not dispatch another send.

The CLI and HTTP API use the same service transition and error composition.
The canonical command references are
[message reply](https://skrzynka.wisent.com/docs/cli/message/reply) and
[message send](https://skrzynka.wisent.com/docs/cli/message/send).

## Real database refusal qualification

`tests/delivery/persistence.rs`, registered as `delivery-persistence`, runs the
actual CLI against a dedicated, quiescent fleet PostgreSQL database containing
real imported mail. It does not send mail. An absent Skarbiec executable causes
an actual credential-resolution refusal; a PostgreSQL check constraint limited
to the journey's unique idempotency key refuses the subsequent `failed` update.
Both reply and outbound paths must retain both causes, and direct database
readback must show the refused terminal write did not change `sending`.

Declare these environment inputs in the qualification environment:

| Variable | Required value |
| --- | --- |
| `SKRZYNKA_DELIVERY_TEST_REVISION` | Exact candidate source revision |
| `SKRZYNKA_DELIVERY_TEST_FLEET_HOME` | Fleet home resolving the dedicated database |
| `SKRZYNKA_DELIVERY_TEST_DATABASE` | Actual dedicated PostgreSQL database name |
| `SKRZYNKA_DELIVERY_TEST_PURPOSE` | `isolated-qualification` |
| `SKRZYNKA_DELIVERY_TEST_ORGANIZATION` | Organization owning the imported message |
| `SKRZYNKA_DELIVERY_TEST_MESSAGE` | Real imported message UUID |
| `SKRZYNKA_DELIVERY_TEST_RECIPIENT` | Valid recipient address for request admission |

The production database name is refused. Credential-file and direct database
URL overrides are refused so CLI execution and database readback use the same
fleet route. Never point this qualification at operator data.

Run with the repository's test tools when execution is authorized:

```sh
cargo test --test delivery-persistence -- --ignored
```

Reports remain under `.build/real-tests/delivery/<run>/`: candidate identity,
command arguments, exit statuses, stdout, stderr, observed refusal rows, and
`report.json`. Cleanup removes only the owned constraint and attempt row; a
cleanup failure is reported alongside the original failure. The body file is
retained with the report.

This journey qualifies credential-failure plus database-refusal diagnostics.
It does not qualify SMTP acceptance, provider uncertainty, worker termination,
post-update readback failure, or the graphical surface. Those flows require
separate real execution and retained evidence; this test's result must not be
reported as full delivery qualification.
