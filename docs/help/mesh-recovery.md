# Mesh recovery runbook

Disaster recovery for an MDE Nebula mesh — full-mesh loss, single-peer
loss, lighthouse failover, and CA backup / restore.

This is the doc the panel's toast emitters point to (NF-16) when a
CA rotation fails or a peer's cert expires. Read it once when you set
up the mesh; come back to it when something has gone wrong.

## CA backup is the most important pre-disaster step

The Certificate Authority private key lives on the lighthouse (the
peer that minted the mesh). If that peer dies and you have no backup,
**no new peers can join** and existing peers will eventually expire.
Back the CA up now.

```bash
# On the lighthouse:
mackesd ca export > ~/mde-ca-backup.enc
```

`ca export` prompts for a passphrase and emits an encrypted bundle
containing the sealed CA private key, the CA cert, the mesh-id, and
every signed peer cert. Store the bundle off-machine — a USB stick in
a safe, a password-manager attachment, encrypted cloud storage.

The NF-18.4 automated backup writes the same bundle every 24 hours to
`~/.mde-mesh/<lighthouse_id>/mackesd/state-backup.enc` (v5+;
legacy installs may still write to `~/QNM-Shared/<lighthouse_id>/mackesd/ca-backup.enc`
per GF-9.1 rename — `mackesd ca import` reads both paths). Until that
worker ships, run `ca export` manually after every CA rotation
or new peer enrollment.

## Full-mesh loss recovery

The lighthouse is dead and you have a CA backup.

```bash
# On a candidate new lighthouse:
mackesd ca import < ~/mde-ca-backup.enc

# Sanity-check:
mackesd ca list           # should show every previously-enrolled peer
mackesd ca dump-ca        # should show the original mesh-id

# Start the lighthouse role:
sudo systemctl restart nebula-lighthouse.service
sudo systemctl restart mackes-nebula-https-tunnel.service
```

Every previously-enrolled peer should reconnect within one heartbeat
cycle once the new lighthouse's overlay IP is reachable. Peers that
were enrolled with an explicit `lighthouse-config.yaml` pointing at
the old lighthouse's public IP will need that config updated to
point at the new one — `mackesd nebula peer-list` on each peer
confirms whether they re-connected.

## Full-mesh loss with no CA backup

This is the worst case. The CA private key is gone, so old peer
certs can never be re-signed under the same chain. You must mint a
new mesh and re-enroll every peer.

```bash
# On the new lighthouse:
mackesd ca mint --mesh-id <new-or-original-name>
# Print the join token:
mackesd nebula peer-list      # empty until peers re-enroll

# On every peer:
mackesd ca import is NOT useful here (no source).
mackesd enroll --token '<fresh-join-token>'
```

You'll lose all audit trail continuity between the old and new mesh.
Historical event rows still resolve in each peer's local SQLite store;
they just don't link to the new mesh-id.

**Don't let this be the recovery path.** Back the CA up.

## Single-peer loss

A peer is dead or unrecoverable. The mesh is fine, you just need to
revoke the dead peer's cert and (optionally) re-enroll a replacement.

```bash
# On the lighthouse:
mackesd ca revoke <dead-node-id>
mackesd decommission <dead-node-id>

# Optional: re-enroll the replacement under the same hostname
# (gets a fresh cert + a new overlay IP)
# On the replacement peer:
mackesd enroll --token '<fresh-join-token>'
```

The CRL propagates to every active peer within one `nebula_supervisor`
tick. The dead peer's overlay IP is freed and may be reassigned to a
future enrollment.

## Lighthouse failover (planned)

The current lighthouse is fine but you want to move the role to a
different peer (e.g. the existing lighthouse is going offline for
maintenance, or you're shifting roles to a peer with a better
public IP).

```bash
# 1. On the new lighthouse candidate: install Nebula + start mackesd.
# 2. On the current lighthouse: export the CA.
mackesd ca export > /tmp/ca.enc

# 3. Transfer to the new lighthouse over the mesh (mesh://).
scp /tmp/ca.enc <new-lighthouse>.mesh:/tmp/

# 4. On the new lighthouse: import.
mackesd ca import < /tmp/ca.enc
sudo systemctl restart nebula-lighthouse.service

# 5. Sign the new lighthouse's cert with the lighthouse group.
mackesd ca sign <new-lighthouse-node-id> --groups lighthouse,peer

# 6. Update every other peer's Nebula config to add the new
#    lighthouse to lighthouse.hosts. Workbench → Network → Mesh →
#    Lighthouses lets you do this without editing /etc/nebula by hand.

# 7. Once all peers are connected via the new lighthouse, demote the
#    old one:
mackesd ca sign <old-lighthouse-node-id> --groups peer
# (on the old lighthouse:)
sudo systemctl stop nebula-lighthouse.service
sudo systemctl stop mackes-nebula-https-tunnel.service

# 8. (Optional) decommission the old lighthouse entirely:
mackesd ca revoke <old-lighthouse-node-id>
mackesd decommission <old-lighthouse-node-id>
```

The mesh stays connected throughout — peers don't notice the
lighthouse swap unless their direct UDP path was relying on the
old one as a relay.

## Cert expiry recovery (the toast pointed me here)

A peer's cert expired. The peer can't reach the overlay until it has
a fresh cert.

### If the lighthouse is reachable

```bash
# On the lighthouse:
mackesd ca sign <expired-peer-node-id>
# The signed bundle propagates via the MDE-Workgroup coordination
# root (~/.mde-mesh/ on v5+; ~/QNM-Shared/ on legacy installs) to
# the expired peer on the next heartbeat tick (≤ 10 s).
```

### If the expired peer can't reach the lighthouse (cert expired
### before the lighthouse rotated)

The peer's overlay connectivity is dead, so it can't pull the new
bundle through the mesh. Use the out-of-band path:

```bash
# On the lighthouse:
mackesd ca sign <expired-peer-node-id>
# Export just the new bundle (CLI shim until NF-18.2 lands the
# typed roster-export):
# MDE-Workgroup coordination root: ~/.mde-mesh/ on v5+; ~/QNM-Shared/ on legacy
sudo cp ~/.mde-mesh/<lighthouse>/mackesd/bundle.json /tmp/ \
    2>/dev/null || sudo cp ~/QNM-Shared/<lighthouse>/mackesd/bundle.json /tmp/

# Sneakernet or scp over LAN to the expired peer:
scp /tmp/bundle.json <expired-peer-LAN-ip>:/tmp/

# On the expired peer (write to the canonical v5+ path):
sudo cp /tmp/bundle.json ~/.mde-mesh/<lighthouse>/mackesd/
sudo systemctl restart mackesd.service
# nebula_supervisor picks up the new bundle on its next tick.
```

## CA rotation failed (the toast pointed me here)

`mackesd ca rotate` aborted mid-way. State of the world depends on
how far the rotation got:

```bash
# Check the audit log on the lighthouse:
mackesd events --kind=nebula_ca_rotated --since '15 min ago'
mackesd logs --kind=audit | tail -20

# If no nebula_ca_rotated event landed:
#   → the rotation didn't start; the old CA is still active. Retry:
mackesd ca rotate

# If a nebula_ca_rotated event landed but only some peers got new
# certs (check mackesd nebula peer-list — look for cert_epoch
# mismatch):
mackesd ca sign <node-id>     # for each lagging peer
# nebula_supervisor pushes the new bundle on the next tick.
```

Don't run `ca rotate` twice in quick succession — the second run
bumps the epoch again and may strand peers that haven't caught up
to the first rotation yet.

## Split-brain (two lighthouses both think they're leader)

See [mesh-ops.md § Recovering from split-brain](mesh-ops.md).
Recovery is `mackesd take-leadership --force` on the desired leader
+ `mackesd yield-leadership` on the other.

## Audit chain break

`mackesd audit verify` reports a `BREAK at event <id>` finding. This
means the SQLite event store was tampered with directly or there's
a daemon bug. Treat as a serious incident:

```bash
# Stop accepting new changes until you understand what happened.
sudo systemctl stop mackesd.service

# Capture the audit log + sqlite db for forensics:
sudo cp ~/.local/share/mde/mackesd.sqlite /tmp/mackesd-snapshot.sqlite
mackesd events --since '24 hours ago' > /tmp/last-24h-events.jsonl

# Verify against the MDE-Workgroup replica (every peer keeps a copy):
ls ~/.mde-mesh/*/mackesd/heartbeat.json 2>/dev/null || ls ~/QNM-Shared/*/mackesd/heartbeat.json
# Compare event sequence numbers across peers — the legitimate
# chain matches; the tampered one doesn't.

# Once you've identified the tampering point, restore the last
# known-good snapshot (created automatically by the daily backup):
sudo cp ~/.mde-mesh/<peer>/mackesd/snapshot-<date>.sqlite \
        ~/.local/share/mde/mackesd.sqlite
# (Legacy install path: ~/QNM-Shared/<peer>/mackesd/snapshot-<date>.sqlite)
sudo systemctl start mackesd.service
mackesd audit verify          # should now return "chain intact"
```

If the audit chain was tampered with by an attacker (not just a
daemon bug), assume the CA may have been compromised — rotate the
CA + revoke every peer that was online during the suspect window.

## What `mackesd ca import` doesn't restore

The CA bundle covers cert chain state only. It does NOT include:

- The per-peer `mackesd.sqlite` event log (each peer keeps its own).
- The MDE-Workgroup coordination root file content
  (`~/.mde-mesh/` on v5+; legacy `~/QNM-Shared/` on pre-v5
  installs). Separate backup via your usual filesystem-level
  tooling.
- Per-peer SSH host keys (regenerated automatically on first boot).
- Per-peer mDNS / hostname state (configured per-machine).

For a fully reproducible mesh disaster-recovery story, pair `mackesd
ca export` with snapshots of the MDE-Workgroup coordination root
(`~/.mde-mesh/`; legacy `~/QNM-Shared/`) + per-peer sqlite databases.

## References

- [mesh-nebula.md](mesh-nebula.md) — overview of the mesh fabric.
- [mesh-admin.md](mesh-admin.md) — day-2 CA operations.
- [mesh-ops.md](mesh-ops.md) — operator runbook for enrollment,
  decommission, audit log reading.
- `docs/design/v2.5-nebula-fabric.md` — architectural notes.
