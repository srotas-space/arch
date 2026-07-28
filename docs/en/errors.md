# Errors

## Description

Every failure returns the same envelope: a single `error` object with a stable
machine-readable `code` and a human-readable `message`. Branch on `code`, never
on `message` — messages are written for people and change without notice.

### Status codes

| Code | Meaning | Retry? |
| --- | --- | --- |
| `400` | The request is malformed or a field is invalid | No — fix the request |
| `401` | Credentials missing, malformed, or unrecognised | No — fix the credentials |
| `403` | Credentials valid, but this caller may not do that | No |
| `404` | The path or referenced object does not exist | No |
| `409` | Conflicts with the current state of the object | No — re-read state first |
| `422` | Well-formed but semantically rejected | No |
| `429` | Rate limit exceeded | Yes — after `Retry-After` |
| `500` | Something failed on our side | Yes — with backoff |
| `503` | Temporarily unavailable or in maintenance | Yes — after `Retry-After` |

### Error codes

| `code` | Status | Cause |
| --- | --- | --- |
| `invalid_request` | 400 | Missing required field, or a field of the wrong type |
| `unauthorized` | 401 | No valid key provided |
| `forbidden` | 403 | The key lacks the scope for this call |
| `not_found` | 404 | Unknown path, or an id that does not resolve |
| `conflict` | 409 | The object changed underneath the request |
| `rate_limited` | 429 | Quota exhausted for the current window |
| `internal_error` | 500 | Unhandled failure — safe to retry |

### What is safe to retry

`4xx` responses describe something about *your* request, and repeating it
unchanged produces the same result. The two exceptions are `429`, which is a
timing problem, and `409`, which means you should re-read state rather than
retry.

`5xx` responses are safe to retry with exponential backoff and jitter. Send an
idempotency key on writes so a retry after a timeout returns the original result
instead of creating a duplicate.

### Handling failures well

- Log `code` and `request_id` together — that pair is what support needs
- Treat an unrecognised `code` as a generic failure rather than crashing; new
  codes are added over time
- Back off with jitter. A fleet that all retries after exactly 30 seconds will
  collide again at second 30

## Architecture

### Arch

```
[Request]
   |
   v
[Validate] ------400--> invalid_request
   |
   v
[Authenticate] --401--> unauthorized
   |
   v
[Authorize] -----403--> forbidden
   |
   v
[Rate limit] ----429--> rate_limited  (Retry-After)
   |
   v
[Handle] --------500--> internal_error
   |
   +--------------200--> result
```

### JSON

#### Response 400 — Invalid request

```json
{
  "error": {
    "code": "invalid_request",
    "field": "name",
    "message": "name is required"
  }
}
```

#### Response 404 — Not found

```json
{
  "error": {
    "code": "not_found",
    "message": "No resource with that id"
  }
}
```

#### Response 429 — Rate limited

Honour `Retry-After` rather than retrying immediately.

```json
{
  "error": {
    "code": "rate_limited",
    "retry_after_seconds": 30,
    "message": "Request quota exhausted for this window"
  }
}
```

#### Response 500 — Internal error

```json
{
  "error": {
    "code": "internal_error",
    "request_id": "req_01hxyz",
    "message": "Unexpected failure; the request was not processed"
  }
}
```

### Text

Errors always arrive as one `error` object. Switch on `code`, log it alongside
`request_id`, and retry only `429`, `500`, and `503` — `429` after `Retry-After`,
the others with exponential backoff.
