# Rate limits

## Description

Quotas are counted per API key over a sliding 60-second window. Every key in an
account draws on the same account budget, so one noisy integration can exhaust
the allowance for the rest — which is why the limit headers are worth reading on
success as well as on failure.

### Limits

| Environment | Requests / minute | Burst |
| --- | --- | --- |
| Test | 60 | 20 |
| Production | 600 | 120 |

### Response headers

Returned on every response, not only on `429`:

| Header | Meaning |
| --- | --- |
| `X-RateLimit-Limit` | Requests permitted in the current window |
| `X-RateLimit-Remaining` | Requests still available |
| `X-RateLimit-Reset` | Unix seconds at which the window resets |
| `Retry-After` | Seconds to wait — sent only with `429` |

### Staying under the limit

Read `X-RateLimit-Remaining` and slow down as it approaches zero rather than
waiting to be refused. When you are refused, wait for `Retry-After` before
retrying; retrying sooner consumes budget without doing work and pushes the
reset further out.

Exponential backoff with jitter matters more than the base delay — without
jitter, every client that was throttled together retries together.

## Architecture

### Arch

```
[Client A] --+
             |
[Client B] --+--> [Account bucket: 600/min] --+--> under -> handle request
             |                                |
[Client C] --+                                +--> over  -> 429 + Retry-After
```

### JSON

#### Response 200 — Headers on a successful call

```json
{
  "headers": {
    "X-RateLimit-Limit": "600",
    "X-RateLimit-Remaining": "417",
    "X-RateLimit-Reset": "1735689600"
  }
}
```

#### Response 429 — Rate limited

```json
{
  "error": {
    "code": "rate_limited",
    "retry_after_seconds": 30,
    "message": "Request quota exhausted for this window"
  }
}
```

### Text

Limits are per key over a rolling minute, drawn from a shared account budget.
Watch `X-RateLimit-Remaining` and throttle before you are refused. On a `429`,
wait `Retry-After` seconds and back off exponentially with jitter.
