# Monitoring

## Description

Metrics, logs, tracing, and alerting coverage.

## Architecture

### Arch

```
[Services] -> [Metrics] -> [Dashboards]
          |-> [Logs] -> [Search]
          |-> [Traces] -> [APM]
```

### JSON

```json
{
  "metrics": "Prometheus",
  "logs": "Loki",
  "traces": "Tempo",
  "alerting": "PagerDuty"
}
```

### Text

All services emit metrics and structured logs. Traces provide request‑level visibility
and alerts are tied to SLO thresholds.
