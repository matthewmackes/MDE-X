# Mesh operator runbook

Day-2 tasks for running an MDE Nebula mesh. Covers enrollment,
decommission, lighthouse operations, split-brain recovery, and diagnostics.

## Enrolling a peer

### From the wizard (GUI)

Workbench → Network → Mesh → **Join existing mesh** → paste join token.

### CLI (headless)

```bash
mackesd enroll --token 'mesh:<mesh_id>@<lighthouse_ip>:4242#<bearer>'
```

Idempotent by hardware fingerprint — re-running refreshes the cert.

### What enrollment does

1. Calls `dev.mackes.MDE.Nebula.Enroll(token)` on the mackesd D-Bus
   surface.
2. The lighthouse signs a cert for this peer and writes a bundle to
   `~/QNM-Shared/<lighthouse_id>/mackesd/bundle.json`.
3. `nebula_supervisor` on the new peer picks up the bundle, writes
   `/etc/nebula/{config.yaml,ca.crt,host.crt,host.key}` atomically,
   and starts `nebula.service`.
4. Peer appears in `mackesd nebula peer-list` within one heartbeat cycle.

## Decommissioning a peer

```bash
mackesd ca revoke <node-id>          # revokes the cert immediately
mackesd decommission <node-id>       # marks the row decommissioned
mackesd decommission <node-id> --force   # if the peer is unreachable
```

The CA CRL is pushed to every active peer within one `nebula_supervisor`
tick. The decommissioned peer loses overlay connectivity immediately.

Historical rows are preserved — audit + topology snapshots that
referenced the node still resolve.

## Rotating the join token

The join token embeds a short-lived bearer. To generate a fresh one:

```bash
mackesd nebula peer-list   # review current peers
# From Workbench: Network → Mesh → + Add peer → copy the new token
```

Old bearers expire; they can't be reused for a second enrollment.

## Recovering from split-brain (two lighthouses both think they're leader)

Symptom: `mackesd healthz` on two peers both report `is_leader: true`.

```bash
# Automatic path: usually resolves within one lease cycle (≤ 60 s)
# as the side with the older applied_revision yields.

# Manual: pick the peer you want to keep as leader
mackesd take-leadership --force     # on the desired leader
mackesd yield-leadership            # on the other peer

# Verify:
mackesd healthz | jq '.is_leader, .applied_revision'
```

## Diagnosing lighthouse health

```bash
# Is this peer's Nebula up?
systemctl status nebula.service
journalctl -u nebula.service -f        # live logs

# Is the lighthouse reachable?
mackesd nebula status                  # shows active_transport

# Overlay connectivity check
ping <peer_overlay_ip>                 # from another peer
```

`active_transport` values:

| Value | Meaning |
|---|---|
| `nebula_direct` | Direct UDP between peers (best case) |
| `nebula_lighthouse_relay` | Traffic relayed via a lighthouse UDP path |
| `nebula_https443` | TLS/443 fallback — UDP blocked, tunnel is active |

## TCP/443 fallback

When direct UDP (port 4242) fails, Nebula's lighthouse runs a
TLS-wrapped TCP/443 listener that relays Nebula frames. This activates
automatically on three consecutive UDP failures.

Operator-visible indicators:

- `mackesd nebula status` → `active_transport: nebula_https443`
- Workbench → Network → Mesh panel → "Mesh in firewall mode" banner

To test the fallback path manually:

```bash
# Block UDP/4242 temporarily (then unblock):
sudo iptables -A OUTPUT -p udp --dport 4242 -j DROP
mackesd nebula status     # should show nebula_https443 within ~30 s
sudo iptables -D OUTPUT -p udp --dport 4242 -j DROP
```

## Reading the audit log

```bash
mackesd logs --kind=audit                        # tail live
mackesd audit verify                             # verify hash chain
mackesd events --node <node-id> --since '2026-05-01'
```

Nebula event kinds in the log:

| Kind | Triggered by |
|---|---|
| `nebula_ca_rotated` | `mackesd ca rotate` |
| `nebula_peer_cert_issued` | new peer enrolled or cert renewed |
| `nebula_peer_cert_revoked` | `mackesd ca revoke <node-id>` |
| `nebula_lighthouse_promoted` | peer promoted to lighthouse |
| `nebula_lighthouse_demoted` | peer demoted from lighthouse |

A failed `audit verify` is a serious finding — do not trust audit data
past the break point.

## Common diagnostics

### Peer shows `unreachable` but ping works

The peer is on the LAN but its Nebula overlay is down. Check:

```bash
ssh <peer> systemctl status nebula.service
ssh <peer> journalctl -u nebula.service -n 50
```

Common causes: `/etc/nebula/host.crt` expired; cert bundle not yet
propagated from the lighthouse; `nebula_supervisor` paused because
`mackesd.service` is down.

### Drift surfaces but never auto-repairs

See the auto-repairable vs manual-review distinction in
[mesh-admin.md](mesh-admin.md). If a drift row carries
`severity = manual-review`, it requires an explicit Approve click in
Workbench → Network → Mesh → Pending Changes.

### Telemetry latency numbers look stale

The `mesh_latency` worker pings every overlay peer every 30 seconds.
If numbers haven't moved in > 2 minutes, the worker is stuck — restart
`mackesd.service` on the affected peer.
