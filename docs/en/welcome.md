# Introduction

## Description

> **This is sample content.** Every page under `docs/` is an example showing what
> the layout can do. Replace it with your own — nothing here is wired into the
> generator.

Describe your API in a sentence or two here: what it does, who it is for, and
what a caller gets back. Keep it short — the pages that follow carry the detail.

All requests go to a single base URL over HTTPS.

```
https://api.example.com
```

### Conventions

| | |
| --- | --- |
| Protocol | HTTPS only |
| Encoding | `application/json`, UTF-8, on request and response |
| Versioning | Major version in the path (`/v1/`) |
| Identifiers | Prefixed and opaque — `res_`, `usr_`. Do not parse them |
| Timestamps | RFC 3339, UTC |

### Making your first call

Send a request with a bearer token, as described in
[Authentication](/en/authentication). The right-hand panel on every page shows
the request, the response, and a ready-to-run `curl` command for it.

### Where to go next

- [Authentication](/en/authentication) — how requests are authorised
- [Resources](/en/resources) — an example endpoint with field tables
- [Errors](/en/errors) — status codes and the error envelope
- [Rate limits](/en/rate-limits) — quotas and the headers that report them

### Writing your own pages

Each page is one Markdown file with a `## Description` section for prose and a
`## Architecture` section holding the `### Arch`, `### JSON`, and `### Text`
tabs. Inside `### JSON`, a `#### Request` / `#### Response` heading turns a code
block into a labelled card. The project readme covers the full format.

## Architecture

### Arch

```
[Client]
    |
    |  HTTPS + bearer token
    v
[API gateway] --> [Service] --> [Datastore]
    |
    +--> [Rate limiter]
```

### JSON

#### Request POST /v1/resources

The smallest call that creates something.

```json
{
  "method": "POST",
  "path": "/v1/resources",
  "headers": {
    "Authorization": "Bearer sk_test_xxxxxxxxxxxx",
    "Content-Type": "application/json"
  },
  "body": {
    "name": "My first resource",
    "type": "standard"
  }
}
```

#### Response 201

```json
{
  "id": "res_01hxyz",
  "name": "My first resource",
  "type": "standard",
  "status": "active",
  "created_at": "2024-01-15T09:30:00Z"
}
```

### Text

Send your bearer token with every request and start with the example endpoint.
Each page's right-hand panel holds the request, the response, and a generated
`curl` sample you can copy and run.
