---
title: Deployment Modes
description: Compare open-source, commercial, and hosted operating models for Wavry.
---

Wavry supports multiple operating models so teams can choose the right balance of control, speed, and licensing fit.

## Summary Table

| Mode | Best Fit | Source/License Considerations | Control Plane Ownership |
|---|---|---|---|
| Open-source self-hosted | Teams prioritizing full control | AGPL-3.0 obligations apply | You own it |
| Commercial license | Closed/private derivative needs | Commercial agreement governs usage | Usually you or hybrid |
| Hosted control plane | Fastest operational start | Service terms apply | Wavry-hosted control plane |

## 1. Open-Source Self-Hosted (AGPL-3.0)

Use this model when you want maximum infrastructure and code control.

### Typical characteristics

- Full code access and modification flexibility
- Full ownership of gateway/relay/runtime deployment
- Strong fit for compliance-heavy environments

### Obligations to evaluate

- AGPL-3.0 distribution/network-use requirements for modified services
- Internal legal review for your distribution and hosting model

## 2. Commercial License

Use this model when AGPL obligations do not fit your product or organizational requirements.

### Typical characteristics

- Closed-source/private derivative distribution support
- Proprietary embedding and internal-only product paths
- Contracted commercial terms and support expectations

### Typical triggers for this model

- Shipping Wavry-derived functionality in proprietary products
- Private forks with restricted source disclosure
- Enterprise procurement/compliance requirements
- Running Wavry as a SaaS offering or deep service integration (direct discussion required)

Commercial pricing details are listed on [Pricing](/pricing).
For SaaS/integration commercial terms, contact `contact@wavry.dev`.

## 3. Official Hosted Services

Use hosted services when you need faster launch and less control-plane operational burden.

### Hosted components can include

- Authentication
- Signaling/matchmaking
- Relay assistance

### Tradeoffs

- Faster onboarding and less infra overhead
- Lower direct control over control-plane operations
- Commercial usage requirements may apply depending on service terms

## Decision Framework

Ask these questions in order:

1. Do you require private/proprietary derivative distribution?
2. Do you need full control-plane ownership for compliance or policy reasons?
3. Is your top priority launch speed over infra ownership?
4. What support/SLA model does your business require?

## Migration Between Modes

Teams often evolve over time:

- Pilot with hosted control plane
- Move to self-hosted once scale/compliance needs increase
- Adopt commercial terms if private derivative constraints emerge

Plan migration with explicit checkpoints for:

- Security controls
- Session routing policy
- Operational ownership and on-call boundaries

## References

- [Pricing](/pricing)
- [COMMERCIAL.md](https://github.com/bybrooklyn/wavry/blob/main/COMMERCIAL.md)
- [TERMS.md](https://github.com/bybrooklyn/wavry/blob/main/TERMS.md)
- [License](https://github.com/bybrooklyn/wavry/blob/main/LICENSE)
