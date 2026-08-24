# Walkthrough: loopback API boundary

This transcript was captured by starting the source-built 0.2.0 service on `127.0.0.1:8791` with the same isolated database and stand-in Skarbiec CLI as the [local lifecycle](walkthrough-local-lifecycle.md). The identity URL was deliberately set to an unreachable loopback HTTPS endpoint. This proves readiness, route exposure, and fail-closed authentication without a real session or network service.

## 1. Start the service

```console
$ skrzynka --database "$STATE/skrzynka.db" serve --bind 127.0.0.1:8791
2026-08-24T23:09:52.159767Z  INFO skrzynka: Skrzynka API ready address=127.0.0.1:8791
```

The log is on stderr; the foreground process continues serving and polling. A non-loopback experiment had already been refused before bind:

```json
{"error":{"code":"NON_LOOPBACK_BIND_REFUSED","message":"Skrzynka serves only loopback addresses","retryable":false}}
```

## 2. Read unauthenticated health

```console
$ curl -i http://127.0.0.1:8791/healthz
HTTP/1.1 200 OK

{"status":"ready","product":"skrzynka","version":"0.2.0","schema_version":2}
```

Health contains no mailbox, identity, or credential data.

## 3. Prove `/v1` fails closed without identity

```console
$ curl -i http://127.0.0.1:8791/v1/status
HTTP/1.1 401 Unauthorized

{"error":{"code":"AUTHENTICATION_REQUIRED","message":"a valid Wisent session is required","retryable":false}}
```

The same response was captured from `POST /v1/sync`; route side effects do not begin before authentication. Supplying a syntactically valid bearer and organization forced central verification, whose isolated endpoint was unavailable:

```console
$ curl -i \
  -H 'Authorization: Bearer lab-session-token' \
  -H 'X-Wisent-Organization-ID: lab-org' \
  http://127.0.0.1:8791/v1/status
HTTP/1.1 502 Bad Gateway

{"error":{"code":"IDENTITY_UNAVAILABLE","message":"central identity verification is unavailable","retryable":true}}
```

The core did not fall back to local trust and returned no counts.

## 4. Exercise the provider callback's safe failure surface

The Gmail callback is intentionally outside bearer middleware because Google invokes it. Without a valid state it returned only generic HTML:

```console
$ curl -i http://127.0.0.1:8791/v1/gmail/oauth/callback
HTTP/1.1 400 Bad Request

<!doctype html><meta charset=utf-8><title>Gmail connection failed</title><p>Gmail could not be connected. Return to Skrzynka Desktop for the exact error.</p>
```

No internal error, token, state, or account was reflected to the browser.

## 5. Use the authenticated resources

With a real Wisent bearer and selected membership, the runnable [API example](../examples/api/use-loopback-api.sh) uses this same service to create a mailbox, synchronize it, and list messages. It writes the bearer to a mode-077 temporary header file, accepts only loopback HTTP, and never places a password in a request. The complete route and schema reference is [API.md](API.md).