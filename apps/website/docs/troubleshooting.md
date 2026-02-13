---
title: Troubleshooting
description: Practical fault-isolation guide with concrete checks and commands.
---

Use this guide to isolate failures quickly.

## Fast Triage (2 Minutes)

Run these first:

```bash
# control-plane container state
docker compose -f docker/control-plane.compose.yml ps

# gateway health endpoint
curl -fsS http://127.0.0.1:3000/health

# recent gateway logs
docker compose -f docker/control-plane.compose.yml logs --tail=100 gateway
```

Then determine if failure is primarily:

- control plane (session setup/auth/routing)
- host runtime (capture/encode/send)
- client runtime (receive/decode/render/input)
- network path (direct route vs relay fallback)

## 1. Cannot Establish Session

Symptoms:

- client never reaches connected state
- handshake errors appear immediately

Checks:

1. confirm gateway is healthy
2. confirm host runtime is active
3. confirm target/session parameters are correct
4. confirm firewall/NAT policy allows required UDP flow

Actions:

- restart control-plane containers
- verify host/client compatibility versions
- retry with minimal topology (single host, single client)

## 2. Session Connects but Feels Laggy

Symptoms:

- high input delay
- visual jitter or stutter

Checks:

1. verify whether session is direct or relay
2. inspect RTT/loss/jitter trends around event time
3. inspect host CPU/GPU saturation
4. reduce bitrate/resolution to isolate bottleneck

Actions:

- restore direct path where possible
- reduce competing host workload
- keep adaptation logs for correlation

## 3. Unexpected High Relay Usage

Symptoms:

- sessions relay in networks where direct path should be common

Checks:

1. validate NAT/firewall behavior
2. confirm candidate address correctness
3. inspect recent infra or policy changes

Actions:

- correct ingress/egress policy for direct path
- verify upstream candidate generation
- compare with known-good baseline region

## 4. Desktop Runtime Issues

Symptoms:

- desktop app starts but runtime does not
- settings do not apply

Checks:

1. run `bun run check` in `crates/wavry-desktop`
2. inspect app logs for command/permission failures
3. verify monitor/audio/input permissions

Linux Wayland/KDE note:

- if `Gdk-Message ... Error 71 (Protocol Error) dispatching to wayland display` appears, run Linux preflight and confirm portal/compositor health.

## 5. Linux Capture and Portal Failures

Checks:

```bash
./scripts/linux-display-smoke.sh
```

If failing:

1. verify `xdg-desktop-portal` + backend package
2. verify PipeWire session state
3. retest with clean login session

Use [Linux and Wayland Support](/linux-wayland-support) for distro-specific recovery flow.

## 6. Audio Issues

Symptoms:

- missing audio
- wrong source selection

Checks:

1. verify selected audio source (`system`, `microphone`, `app:<name>`)
2. verify OS capture permissions
3. verify sample-rate/channel compatibility in logs

## 7. CI/CD Build Failures

Checks:

1. re-run with clean workspace state
2. confirm lockfiles and cache keys are valid
3. verify platform dependencies
4. verify Dockerfile and workflow syntax changes

## 8. Useful Diagnostic Bundle

When escalating an issue, provide:

1. precise timestamp window
2. gateway/relay logs for that window
3. host/client runtime logs
4. whether session was direct or relay
5. environment details (OS/compositor/version)

## Related Docs

- [Getting Started](/getting-started)
- [Operations](/operations)
- [Linux and Wayland Support](/linux-wayland-support)
- [Security](/security)
