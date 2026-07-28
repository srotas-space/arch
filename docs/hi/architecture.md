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
  |  POST /v1/resources
  |--------------------------------------------->|
  |                                              |
Edge / CDN                                        |
  |                                              |
  |  TLS termination, WAF, rate limiting          |
  |--------------------------------------------->|
  |                                              |
API gateway                                       |
  |                                              |
  |  authenticate, authorize, route               |
  |--------------------------------------------->|
  |                                              |
Application service                               |
  |                                              |
  |  validate -> write -> emit event              |
  |---------------------------------------------|
  |                                              |
Primary datastore                                 |
  |                                              |
  |  commit + outbox row                          |
  |--------------------------------------------->|
  |                                              |
Message broker                                    |
  |                                              |
  |  fan out to subscribers                       |
  |---------------------------------------------|
  |                                              |
Workers                     Read replicas         |
  |                          |                   |
  |  async side effects       | serve queries     |
  |-------------------------> |----------------->|
  |                                              |
Client                                            |
  |                                              |
  |  201 Created + resource body                  |
  |----------------------------------------------|
```

### JSON

```json
{
  "flow": "write_request_lifecycle",
  "stages": [
    { "name": "edge", "role": "tls_termination", "protects": ["waf", "rate_limit"] },
    { "name": "gateway", "role": "auth", "checks": ["token", "scope"] },
    { "name": "service", "role": "handle", "steps": ["validate", "persist", "emit"] },
    { "name": "datastore", "role": "commit", "writes": ["record", "outbox"] },
    { "name": "broker", "role": "fanout", "topics": ["resource.created"] },
    { "name": "workers", "role": "async", "handles": ["indexing", "notifications"] }
  ]
}
```

### Text

A write enters through the edge, where TLS terminates and rate limiting applies.
The gateway authenticates the caller and routes to the service, which validates
the request, commits it alongside an outbox row, and returns. The outbox is
published to the broker, where workers pick up asynchronous side effects and
read replicas take query load off the primary.

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
