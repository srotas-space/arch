# Authentication

## Description

Requests are authorised with a bearer token sent in the `Authorization` header.
There are no cookies and no session — every request is authenticated on its own.

```
Authorization: Bearer sk_test_xxxxxxxxxxxx
```

A request without a valid token is refused with `401` before any other
validation runs.

### Key types

| Prefix | Environment | Notes |
| --- | --- | --- |
| `sk_test_` | Test | Safe to experiment with; operates on test data only |
| `sk_live_` | Production | Operates on real data |

### Keeping keys safe

A secret key carries the full authority of the account that owns it. Treat it
the way you would a database password:

- Keep it in a secrets manager or environment variable, never in source control
- Never ship it to a browser, mobile app, or anything else a user can read
- Rotate it on a schedule, and immediately if it may have been exposed

Supporting two active keys at once is what makes rotation possible without
downtime: issue the new key, move traffic onto it, then revoke the old one.

### Authorised but not permitted

A valid token that lacks the scope for an endpoint gets `403`, not `401`. The
distinction matters when you handle failures: `401` means the credential is
wrong and retrying will not help, while `403` means the credential is fine but
this caller may not perform this action. See [Errors](/en/errors).

## Architecture

### Arch

```
[Request]
   |
   |-- Authorization: Bearer ... --> [Verify token]
   |                                       |
   |                            +----------+----------+
   |                            |                     |
   v                         valid                 invalid
[Scope check]                   |                     |
   |                            v                     v
   +--> in scope   -> continue                   401 unauthorized
   |
   +--> out of scope -> 403 forbidden
```

### JSON

#### Request POST /v1/resources

A token on a live call.

```json
{
  "method": "POST",
  "path": "/v1/resources",
  "headers": {
    "Authorization": "Bearer sk_test_xxxxxxxxxxxx",
    "Content-Type": "application/json"
  },
  "body": {
    "name": "My first resource"
  }
}
```

#### Response 401 — Missing or invalid token

```json
{
  "error": {
    "code": "unauthorized",
    "message": "No valid API key provided"
  }
}
```

#### Response 403 — Not permitted

```json
{
  "error": {
    "code": "forbidden",
    "message": "This key does not have access to that resource"
  }
}
```

### Text

Send `Authorization: Bearer <key>` on every request. A `401` means the key
itself is missing or wrong — fix it rather than retrying. A `403` means the key
is valid but lacks permission for this call.
