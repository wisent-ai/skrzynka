# Organization scope

Organization scope is the database namespace selected by a centrally verified Wisent session. It prevents one signed-in organization from observing another organization's mailbox-derived state.

## HTTP authentication

Every `/v1` route except the Gmail provider callback requires:

```http
Authorization: Bearer <wisent-session>
X-Wisent-Organization-ID: <organization-id>
```

Both values must be nonempty; the organization identifier is limited to 128 ASCII letters, digits, `-`, or `_`. The core sends the bearer to the configured Supabase `auth/v1/user` endpoint, then queries `organization_members` for that user and selected organization. It does not trust a desktop assertion by itself.

`GET /healthz` is unauthenticated and contains only readiness and versions. The OAuth callback is unauthenticated because Google owns that browser redirect; it returns generic HTML and resolves the flow's stored organization internally.

## Durable boundary

Mailbox rows store `organization_id`. All public mailbox, message, and reply SQL either filters it directly or joins back through the mailbox. The field is not serialized. A missing resource and a resource in another organization both return `NOT_FOUND`, so membership cannot probe foreign UUIDs.

CLI commands do not impersonate a Wisent account. They consistently use the explicit `legacy-local` organization. Schema migration 1→2 assigns existing rows to that namespace.

## Refusals

- Missing/malformed bearer or organization header: HTTP 401, `AUTHENTICATION_REQUIRED` — `a valid Wisent session is required`.
- Valid session without selected membership: HTTP 403, `ORGANIZATION_ACCESS_DENIED` — `the signed-in account does not belong to the selected organization`.
- Identity transport, non-success response, or malformed identity payload: HTTP 502, `IDENTITY_UNAVAILABLE` — `central identity verification is unavailable` (`retryable:true`).

Identity verification fails closed before database access. Shared identity owns user sessions and memberships; Skrzynka owns only their enforcement at this local API boundary.