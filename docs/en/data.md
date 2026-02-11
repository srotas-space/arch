# Data

## Description

Primary data services and durability strategy.

## Architecture

### Arch

```
[Services] -> [Primary DB] -> [Read Replicas]
          |-> [Cache]
          |-> [Object Storage]
```

### JSON

```json
{
  "database": "Aurora",
  "replicas": 2,
  "cache": "Redis",
  "storage": "S3"
}
```

### Text

Write traffic is isolated to the primary DB. Reads are offloaded to replicas and
hot paths use Redis for low‑latency access.
