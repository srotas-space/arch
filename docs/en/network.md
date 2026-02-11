# Network

## Description

Network boundaries, ingress/egress, and private routing rules across environments.

## Architecture

### Arch

```
[Internet] -> [WAF] -> [ALB] -> [Private Subnets] -> [Services]
                         |-> [NAT Gateway] -> [External APIs]
```

### JSON

```json
{
  "components": ["WAF", "ALB", "NAT", "VPC", "Private Subnets"],
  "ingress": "HTTPS",
  "egress": "NAT",
  "zones": 3
}
```

### Text

Ingress is protected by WAF and routed through ALB. Services are isolated in private subnets.
Outbound traffic is centralized through NAT gateways with egress allow‑lists.
