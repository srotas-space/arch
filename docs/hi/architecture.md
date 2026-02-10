# Architecture Overview

## Description

<div class="env-grid">
  <div class="env-card">
    <div class="env-header">
      <div class="env-title">Test</div>
      <span class="env-tag env-tag-test">Low Risk</span>
    </div>
    <div class="env-meta">Smaller footprint · short-lived data</div>
    <div class="env-costs">
      <span class="cost-chip">$38/day</span>
      <span class="cost-chip">$1.14k/month</span>
    </div>
    <div class="env-services">
      <span>2x t3.medium</span>
      <span>RDS micro</span>
      <span>S3 + CDN</span>
      <span>Basic logs</span>
    </div>
  </div>
  <div class="env-card">
    <div class="env-header">
      <div class="env-title">Dev</div>
      <span class="env-tag env-tag-dev">Shared</span>
    </div>
    <div class="env-meta">Shared services · CI-heavy</div>
    <div class="env-costs">
      <span class="cost-chip">$74/day</span>
      <span class="cost-chip">$2.22k/month</span>
    </div>
    <div class="env-services">
      <span>4x t3.large</span>
      <span>RDS small</span>
      <span>Redis cache</span>
      <span>Logs + metrics</span>
    </div>
  </div>
  <div class="env-card">
    <div class="env-header">
      <div class="env-title">Staging</div>
      <span class="env-tag env-tag-stage">Prod-like</span>
    </div>
    <div class="env-meta">Prod-like · full monitoring</div>
    <div class="env-costs">
      <span class="cost-chip">$156/day</span>
      <span class="cost-chip">$4.68k/month</span>
    </div>
    <div class="env-services">
      <span>8x m5.large</span>
      <span>RDS multi-AZ</span>
      <span>Kafka cluster</span>
      <span>Full tracing</span>
    </div>
  </div>
  <div class="env-card">
    <div class="env-header">
      <div class="env-title">Prod</div>
      <span class="env-tag env-tag-prod">Critical</span>
    </div>
    <div class="env-meta">High availability · multi-AZ</div>
    <div class="env-costs">
      <span class="cost-chip">$420/day</span>
      <span class="cost-chip">$12.6k/month</span>
    </div>
    <div class="env-services">
      <span>24x m6i.xlarge</span>
      <span>Aurora multi-AZ</span>
      <span>Private link</span>
      <span>24/7 on-call</span>
    </div>
  </div>
</div>

## Architecture

Below is the environment stack, service layout, and cost breakdown.

### Arch

```
Client
  |
  |  (already accepted; synthetic order exists)
  |
  |----------------------------------------------|
  |                                              |
trading-core (Tier-1)                             |
  |                                              |
  |  cmd.venue.place.v1 (qty=3.0, attempt=EA-1)  |
  |--------------------------------------------->|
  |                                              |
venue-router-adapters (Tier-2)                    |
  |                                              |
  |  place order on venue                        |
  |--------------------------------------------->|
  |                                              |
External Venue                                   |
  |                                              |
  |  partial fill: 1.2 BTC @ 9980                |
  |---------------------------------------------|
  |                                              |
venue-router-adapters                             |
  |                                              |
  |  evt.venue.execution.report.v1               |
  |  (filled=1.2, remaining=1.8, EA-1)           |
  |--------------------------------------------->|
  |                                              |
trading-core                                     |
  |                                              |
  |  atomic FDB txn:                             |
  |   - update synthetic order                  |
  |   - ledger postings                         |
  |   - position update                         |
  |   - write outbox                            |
  |---------------------------------------------|
  |                                              |
Outbox Publisher                                 |
  |                                              |
  |  publish evt.trading.*, evt.ledger.*         |
  |--------------------------------------------->|
  |                                              |
Kafka                                            |
  |                                              |
  |  fanout                                      |
  |---------------------------------------------|
  |                                              |
ws-gateway                  edge-api             |
  |                          |                   |
  |  push deltas              | read snapshots   |
  |-------------------------> |----------------->|
  |                                              |
Client UI                                        |
  |                                              |
  |  sees: 1.2 / 3 BTC filled                    |
  |                                              |
  |----------------------------------------------|
  |                                              |
trading-core                                     |
  |                                              |
  |  cmd.venue.place.v1 (qty=1.8, attempt=EA-2)  |
  |--------------------------------------------->|
  |              (loop Steps 6 → 9)              |
  |                                              |
```

### JSON

```json
{
  "flow": "order_lifecycle",
  "stages": [
    { "name": "client", "event": "cmd.venue.place.v1", "qty": 3.0, "attempt": "EA-1" },
    { "name": "venue", "event": "partial_fill", "filled": 1.2, "price": 9980 },
    { "name": "trading_core", "event": "txn.commit", "updates": ["synthetic_order", "ledger", "position", "outbox"] },
    { "name": "outbox", "event": "publish", "topics": ["evt.trading.*", "evt.ledger.*"] },
    { "name": "client_ui", "event": "update", "filled": "1.2/3" }
  ]
}
```

### Text

The client places a synthetic order that is routed to a venue adapter. The venue returns a partial fill,
trading-core commits an atomic transaction in FoundationDB, and the outbox publishes events to Kafka.
Realtime deltas flow to the UI through ws-gateway, while edge-api serves snapshots. The remaining quantity
is re-submitted as a second attempt until the order completes.

## Cost breakdown by environment (daily / monthly)

| Environment | Compute | Data | Network | Observability | Total (daily) | Total (monthly) |
| --- | --- | --- | --- | --- | --- | --- |
| Test | $14 | $9 | $5 | $10 | $38 | $1.14k |
| Dev | $28 | $18 | $8 | $20 | $74 | $2.22k |
| Staging | $62 | $42 | $18 | $34 | $156 | $4.68k |
| Prod | $180 | $120 | $60 | $60 | $420 | $12.6k |

## Architecture stack (per environment)

<div class="aws-grid">
  <div class="aws-stack">
    <div class="stack-head">
      <span class="stack-icon">☁️</span>
      <div>
        <h3>Network</h3>
        <p>VPC, subnets, routing, ingress control</p>
      </div>
    </div>
    <div class="stack-body">
      <div class="stack-chip">VPC + CIDR</div>
      <div class="stack-chip">Private subnets</div>
      <div class="stack-chip">NAT + egress</div>
      <div class="stack-chip">WAF rules</div>
    </div>
  </div>
  <div class="aws-stack">
    <div class="stack-head">
      <span class="stack-icon">⚡</span>
      <div>
        <h3>Compute</h3>
        <p>Autoscaling apps, containers, background jobs</p>
      </div>
    </div>
    <div class="stack-body">
      <div class="stack-chip">ASG + ALB</div>
      <div class="stack-chip">ECS services</div>
      <div class="stack-chip">Batch workers</div>
      <div class="stack-chip">Spot strategy</div>
    </div>
  </div>
  <div class="aws-stack">
    <div class="stack-head">
      <span class="stack-icon">🗄️</span>
      <div>
        <h3>Data</h3>
        <p>Managed storage, cache, and streaming</p>
      </div>
    </div>
    <div class="stack-body">
      <div class="stack-chip">Aurora / RDS</div>
      <div class="stack-chip">Redis</div>
      <div class="stack-chip">S3 + CDN</div>
      <div class="stack-chip">Kafka / MSK</div>
    </div>
  </div>
  <div class="aws-stack">
    <div class="stack-head">
      <span class="stack-icon">📊</span>
      <div>
        <h3>Observability</h3>
        <p>Metrics, traces, alerting, dashboards</p>
      </div>
    </div>
    <div class="stack-body">
      <div class="stack-chip">Prom + Grafana</div>
      <div class="stack-chip">Trace sampling</div>
      <div class="stack-chip">SLO alerts</div>
      <div class="stack-chip">Log pipelines</div>
    </div>
  </div>
</div>

## Environment lanes

<div class="lane-grid">
  <div class="lane-card">
    <div class="lane-title">Test</div>
    <div class="lane-body">
      <div class="lane-pill">1 AZ</div>
      <div class="lane-pill">No DR</div>
      <div class="lane-pill">Ephemeral data</div>
    </div>
  </div>
  <div class="lane-card">
    <div class="lane-title">Dev</div>
    <div class="lane-body">
      <div class="lane-pill">2 AZ</div>
      <div class="lane-pill">Nightly snapshots</div>
      <div class="lane-pill">Shared tooling</div>
    </div>
  </div>
  <div class="lane-card">
    <div class="lane-title">Staging</div>
    <div class="lane-body">
      <div class="lane-pill">2 AZ</div>
      <div class="lane-pill">Prod parity</div>
      <div class="lane-pill">Full tracing</div>
    </div>
  </div>
  <div class="lane-card">
    <div class="lane-title">Prod</div>
    <div class="lane-body">
      <div class="lane-pill">3 AZ</div>
      <div class="lane-pill">DR ready</div>
      <div class="lane-pill">24/7 on-call</div>
    </div>
  </div>
</div>
