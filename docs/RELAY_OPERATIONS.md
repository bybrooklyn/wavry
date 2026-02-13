# Relay Server Operations Guide

**Version:** 0.0.5-unstable  
**Last Updated:** 2026-02-13

This document provides operational guidance for deploying and managing Wavry relay servers.

---

## Table of Contents

1. [Deployment](#deployment)
2. [Monitoring](#monitoring)
3. [Troubleshooting](#troubleshooting)
4. [Incident Response](#incident-response)
5. [Capacity Planning](#capacity-planning)
6. [Security Best Practices](#security-best-practices)

---

## Deployment

### Prerequisites

- Linux server with public IPv4 address
- Minimum 1 vCPU, 512MB RAM
- UDP port accessibility (default: 4000)
- Docker (recommended) or Rust toolchain

### Docker Deployment (Recommended)

```bash
# Pull the latest relay image
docker pull ghcr.io/bybrooklyn/wavry-relay:latest

# Run with environment configuration
docker run -d \
  --name wavry-relay \
  --restart unless-stopped \
  -p 4000:4000/udp \
  -p 9091:9091/tcp \
  -e WAVRY_MASTER_URL=https://master.wavry.dev \
  -e WAVRY_RELAY_ALLOW_PUBLIC_BIND=1 \
  -e WAVRY_RELAY_MASTER_PUBLIC_KEY=<master_public_key_hex> \
  -e WAVRY_RELAY_MAX_BITRATE=20000 \
  -e WAVRY_RELAY_REGION=us-east-1 \
  ghcr.io/bybrooklyn/wavry-relay:latest
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `WAVRY_RELAY_LISTEN` | `0.0.0.0:4000` | UDP listen address |
| `WAVRY_MASTER_URL` | `http://localhost:8080` | Master server URL |
| `WAVRY_RELAY_MASTER_PUBLIC_KEY` | None | Ed25519 public key (hex) from Master |
| `WAVRY_RELAY_MASTER_TOKEN` | None | Bearer token for authenticated relay register/heartbeat requests |
| `WAVRY_RELAY_ALLOW_PUBLIC_BIND` | `0` | Allow binding to public IPs (required in production) |
| `WAVRY_RELAY_ALLOW_INSECURE_DEV` | `0` | Skip signature validation (dev only, never use in prod) |
| `WAVRY_RELAY_REGION` | None | Geographic region (e.g., `us-east-1`, `eu-west-1`) |
| `WAVRY_RELAY_ASN` | None | Autonomous System Number |
| `WAVRY_RELAY_MAX_BITRATE` | `20000` | Maximum supported bitrate in kbps |

### Binary Deployment

```bash
# Build from source
cargo build --release --package wavry-relay

# Run with configuration
./target/release/wavry-relay \
  --listen 0.0.0.0:4000 \
  --master-url https://master.wavry.dev \
  --master-public-key <hex_key> \
  --max-sessions 100 \
  --region us-east-1
```

---

## Monitoring

### Health Endpoints

The relay exposes HTTP endpoints on port 9091 (configurable via `--health-listen`):

#### `/health`
Basic health check with active session counts.

```bash
curl http://localhost:9091/health
```

Response:
```json
{
  "relay_id": "550e8400-e29b-41d4-a716-446655440000",
  "active_sessions": 12,
  "max_sessions": 100,
  "uptime_secs": 3600,
  "metrics": { ... }
}
```

#### `/ready`
Kubernetes-style readiness probe. Returns 200 if ready, 503 if not.

```bash
curl http://localhost:9091/ready
```

#### `/metrics` (JSON)
Detailed metrics in JSON format.

```bash
curl http://localhost:9091/metrics
```

#### `/metrics/prometheus` (New!)
Prometheus-compatible metrics endpoint for monitoring systems.

```bash
curl http://localhost:9091/metrics/prometheus
```

Example output:
```
# HELP wavry_relay_packets_rx Total packets received
# TYPE wavry_relay_packets_rx counter
wavry_relay_packets_rx{relay_id="..."} 123456
# HELP wavry_relay_bytes_forwarded Total bytes forwarded
# TYPE wavry_relay_bytes_forwarded counter
wavry_relay_bytes_forwarded{relay_id="..."} 98765432
```

### Key Metrics to Monitor

| Metric | Description | Alert Threshold |
|--------|-------------|-----------------|
| `packets_rx` | Total packets received | N/A (counter) |
| `packets_forwarded` | Successfully forwarded packets | < 95% of packets_rx |
| `dropped_packets` | Packets dropped (all reasons) | > 5% of packets_rx |
| `rate_limited_packets` | Packets dropped due to rate limiting | > 1% of packets_rx |
| `invalid_packets` | Malformed packets | > 0.1% of packets_rx |
| `auth_reject_packets` | Failed authentication | Monitor for abuse |
| `session_full_rejects` | Capacity limit reached | > 0 (scale up) |
| `overload_shed_packets` | Load shedding active | > 0 (scale up) |
| `active_sessions` | Current active sessions | > 80% of max_sessions |

### Prometheus Integration

Add to your `prometheus.yml`:

```yaml
scrape_configs:
  - job_name: 'wavry-relay'
    static_configs:
      - targets: ['relay1.example.com:9091', 'relay2.example.com:9091']
    metrics_path: '/metrics/prometheus'
    scrape_interval: 15s
```

### Grafana Dashboard

Key panels to include:
1. **Active Sessions** (gauge) - Current load
2. **Packet Throughput** (rate) - packets_rx and packets_forwarded
3. **Bandwidth** (rate) - bytes_rx and bytes_forwarded
4. **Error Rates** (rate) - dropped_packets, invalid_packets, auth_reject_packets
5. **Session Lifecycle** (rate) - cleanup_expired_sessions, cleanup_idle_sessions
6. **NAT Rebind Events** (rate) - nat_rebind_events

---

## Troubleshooting

### Issue: Relay not registering with Master

**Symptoms:**
- Relay starts but logs repeated "Failed to connect to Master" warnings
- `/ready` endpoint returns 503

**Diagnosis:**
```bash
# Check network connectivity to Master
curl -v https://master.wavry.dev/health

# Check relay logs
docker logs wavry-relay | grep -i "master"
```

**Common Causes:**
1. Incorrect `WAVRY_MASTER_URL`
2. Firewall blocking outbound HTTPS
3. Master server unavailable
4. Invalid master public key

**Resolution:**
- Verify Master URL is correct and accessible
- Ensure outbound HTTPS (443) is allowed
- Check Master server status
- Verify `WAVRY_RELAY_MASTER_PUBLIC_KEY` matches Master's current key

---

### Issue: High packet drop rate

**Symptoms:**
- `dropped_packets` counter increasing rapidly
- `packets_forwarded` << `packets_rx`

**Diagnosis:**
```bash
# Check detailed metrics
curl http://localhost:9091/metrics | jq '.dropped_packets'
curl http://localhost:9091/metrics | jq '.rate_limited_packets'
curl http://localhost:9091/metrics | jq '.invalid_packets'
```

**Common Causes:**
1. Rate limiting triggered (legitimate or attack)
2. Invalid/malformed packets
3. Expired leases
4. Session capacity exceeded

**Resolution:**
- If `rate_limited_packets` high: Increase `--ip-rate-limit-pps` or investigate source IPs
- If `invalid_packets` high: Check for protocol version mismatches or attacks
- If `expired_lease_rejects` high: Verify lease duration settings and Master connectivity
- If `session_full_rejects` high: Increase `--max-sessions` or deploy additional relays

---

### Issue: Memory usage growing unbounded

**Symptoms:**
- Relay memory usage steadily increasing
- Docker container OOM killed

**Diagnosis:**
```bash
# Check active sessions
curl http://localhost:9091/health | jq '.active_sessions'

# Check session cleanup metrics
curl http://localhost:9091/metrics | jq '.cleanup_idle_sessions'
```

**Common Causes:**
1. Session cleanup not running
2. Idle timeout too high
3. Session leaks (bug)

**Resolution:**
- Verify `--cleanup-interval-secs` is set (default: 10s)
- Reduce `--idle-timeout` if needed (default: 60s)
- Restart relay to clear sessions
- Report session leak if confirmed

---

### Issue: Relay behind NAT not accessible

**Symptoms:**
- Relay registers but peers cannot connect
- STUN binding fails

**Diagnosis:**
```bash
# Check relay's reported endpoints
curl http://localhost:9091/health | jq '.endpoints'

# Test UDP reachability from external source
nc -u <relay_public_ip> 4000
```

**Resolution:**
- Ensure UDP port forwarding is configured on NAT device
- Verify firewall allows inbound UDP on relay port
- Consider using a cloud provider with public IP (AWS, GCP, Azure)
- Check STUN server accessibility

---

## Incident Response

### Relay Unresponsive

1. **Check process health:**
   ```bash
   docker ps -a | grep wavry-relay
   docker logs --tail 100 wavry-relay
   ```

2. **Verify network connectivity:**
   ```bash
   curl http://localhost:9091/health
   ```

3. **Check resource limits:**
   ```bash
   docker stats wavry-relay
   ```

4. **Restart if needed:**
   ```bash
   docker restart wavry-relay
   ```

5. **Escalate if persistent:**
   - Collect logs: `docker logs wavry-relay > relay-logs.txt`
   - Check metrics history from monitoring system
   - File issue at https://github.com/bybrooklyn/wavry/issues

---

### Suspected DDoS Attack

1. **Confirm attack pattern:**
   ```bash
   curl http://localhost:9091/metrics | jq '.rate_limited_packets'
   ```

2. **Enable verbose logging temporarily:**
   ```bash
   docker restart wavry-relay -e RUST_LOG=debug
   ```

3. **Analyze source IPs in logs:**
   ```bash
   docker logs wavry-relay | grep "rate limited" | awk '{print $NF}' | sort | uniq -c | sort -rn | head -20
   ```

4. **Consider IP-level blocking:**
   - Use `iptables` or cloud firewall to block abusive IPs
   - Coordinate with Master to ban malicious identities

5. **Scale horizontally:**
   - Deploy additional relay instances
   - Use load balancer with DDoS mitigation

---

## Capacity Planning

### Sizing Guidelines

**Note:** Max Bitrate represents aggregate relay throughput capacity across all sessions, not per-session limits.

| Relay Capacity | vCPU | RAM | Max Sessions | Aggregate Throughput | Network Bandwidth |
|----------------|------|-----|--------------|---------------------|-------------------|
| Small | 1 | 512MB | 50 | 500 Mbps (10 Mbps/session avg) | 100 Mbps+ |
| Medium | 2 | 1GB | 100 | 2 Gbps (20 Mbps/session avg) | 500 Mbps+ |
| Large | 4 | 2GB | 250 | 5 Gbps (20 Mbps/session avg) | 1 Gbps+ |
| XLarge | 8 | 4GB | 500 | 10 Gbps (20 Mbps/session avg) | 10 Gbps+ |

**Per-Session Bitrates:**
- Typical: 5-20 Mbps (1080p60)
- High quality: 20-50 Mbps (4K60)
- Configuration: Set via `--max-bitrate-kbps` (default: 20000 = 20 Mbps)

### When to Scale

**Scale Up (increase resources):**
- Active sessions consistently > 80% of max_sessions
- CPU usage > 70%
- Memory usage > 80%
- `overload_shed_packets` increasing

**Scale Out (add relays):**
- Geographic distribution needed
- Network redundancy required
- Total capacity exceeds single-node limits

---

## Security Best Practices

### Production Deployment

1. **Never use `WAVRY_RELAY_ALLOW_INSECURE_DEV=1` in production**
   - Always require cryptographic lease validation

2. **Rotate Master public key carefully**
   - Update all relays simultaneously
   - Monitor registration failures during rotation

3. **Use dedicated service accounts**
   - Run relay as non-root user
   - Limit file system permissions

4. **Network isolation**
   - Place relays in DMZ or isolated VPC
   - Use security groups to limit inbound traffic

5. **Monitor for abuse**
   - Set alerts on `auth_reject_packets` spikes
   - Track `rate_limited_packets` per source IP
   - Coordinate with Master for identity-level bans

6. **Keep software updated**
   - Subscribe to GitHub releases: https://github.com/bybrooklyn/wavry/releases
   - Apply security patches promptly
   - Test updates in staging before production

---

## Support

- **Documentation:** https://github.com/bybrooklyn/wavry/tree/main/docs
- **Issues:** https://github.com/bybrooklyn/wavry/issues
- **Discord:** (Coming soon)
- **Email:** support@wavry.dev
