# Compute

## Description

Compute layer for app services, background workers, and batch workloads.

## Architecture

### Arch

```
[ALB] -> [ECS Services] -> [Worker Queue]
                 |-> [Autoscaling Policies]
```

### JSON

```json
{
  "services": ["api", "worker", "scheduler"],
  "autoscaling": {
    "min": 3,
    "max": 24,
    "target_cpu": 65
  }
}
```

### Text

Services run in containers with autoscaling tuned for latency. Batch workloads use
separate worker pools to isolate heavy jobs.
