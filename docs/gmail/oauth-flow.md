# Gmail OAuth flow over the API

`skrzynka account authorize --provider gmail` runs this flow in one command.
A client of the loopback API, such as Skrzynka Desktop, runs the same flow in
three calls.

## Start

`POST /v1/gmail/oauth/start` with `{"skarbiec_item_id": "<google-login-item>"}`
needs the administrator role. It answers with the flow id and the
authorization URL to open in the operator's browser:

```json
{"flow_id": "6f0c…", "authorization_url": "https://accounts.google.com/…"}
```

## Wait for the outcome

`GET /v1/gmail/oauth/<flow_id>/settled` answers once Google's redirect has
reached `/v1/gmail/oauth/callback` and the core has completed or failed the
flow. It waits for that callback rather than answering `pending`, so a client
reads the outcome once instead of asking again on a clock. It sets no limit of
its own. A client that gives up closes the connection, and the core drops the
wait with it. The flow itself stays open until a callback settles it.

The answer has the same shape as the status read:

```json
{"flow_id": "6f0c…", "status": "completed", "mailbox": {"id": "…", "email": "user@example.com"}, "error": null}
{"flow_id": "6f0c…", "status": "failed", "mailbox": null, "error": {"code": "GMAIL_OAUTH_REJECTED", "message": "…", "retryable": false}}
```

`completed` carries the mailbox the core declared in Skarbiec. `failed`
carries the refusal the callback reported: declined consent, an invalid code
or a failed token exchange, with the core's own code and sentence.

## Read the current state

`GET /v1/gmail/oauth/<flow_id>` answers at once with the flow's current
state: `pending`, `processing`, `completed` or `failed`.

## Refusals

Both reads refuse a flow id that is not a UUID with `INVALID_IDENTIFIER`. A
flow this core does not hold, or one another organization started, gets the
refusal `NOT_FOUND`, `Gmail OAuth flow was not found`. An unauthenticated call
is refused before the flow is looked up.

## The callback page

The browser lands on `/v1/gmail/oauth/callback`. When the core accepts the
callback, the page says Gmail is connected. When it refuses, the page names
the core's code and sentence, which is the same refusal the settled read
returns.
