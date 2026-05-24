# Mesh admin guide

CA operations, peer cert management, and lighthouse administration
for MDE's Nebula-based mesh.

## CA operations

The certificate authority lives on the first lighthouse. Its private
key is sealed in the mackesd SQLite store and never leaves the machine.

### Mint a new CA (first-boot only)

Handled automatically by the first-boot wizard. To mint manually:

```bash
mackesd ca mint --mesh-id <name> [--cert-lifetime-days 365]
```

Output: CA cert written to `/etc/nebula/ca.crt` and sealed in the store.
The join token printed at the end is what you share with new peers.

### Sign a peer cert

Enrollment does this automatically. To sign manually (e.g. for a
headless peer that lost its cert):

```bash
mackesd ca sign <node-id> [--groups lighthouse,peer]
```

The signed cert is written into the bundle and pushed to the peer's
`nebula_supervisor` on its next heartbeat.

### List all signed certs

```bash
mackesd ca list
```

Prints node-id, overlay IP, cert expiry, groups, and revocation status
for every cert the CA has ever signed.

### Inspect the CA cert

```bash
mackesd ca dump-ca
```

Prints the CA cert in PEM format plus its expiry and the mesh-id it
covers.

### Rotate the CA (epoch bump)

Use when the CA private key may be compromised or as a scheduled
rotation:

```bash
mackesd ca rotate [--cert-lifetime-days 365]
```

What happens:

1. A new CA keypair is minted and sealed.
2. Every enrolled peer gets a fresh cert signed by the new CA.
3. `nebula_supervisor` restarts Nebula on each peer within one heartbeat
   cycle (≤ 10 seconds).
4. An audit event `nebula_ca_rotated` lands in the event log.

Peers that are offline when rotation runs will fail to connect until
they come back online and receive their new cert bundle.

## Peer cert management

### Revoke a peer cert

```bash
mackesd ca revoke <node-id>
```

The peer's cert is added to the Nebula CRL. All active peers reload
their config within one `nebula_supervisor` tick. The revoked peer
loses overlay connectivity immediately.

### Re-enroll a revoked peer

After revoking, use a fresh join token on the peer:

```bash
# On the lighthouse: generate a new token
mackesd nebula peer-list   # confirm the node is revoked

# On the peer:
mackesd enroll --token '<new-join-token>'
```

### Check cert expiry

```bash
mackesd nebula peer-list   # shows cert_expiry for every peer
mackesd nebula status      # shows this peer's own cert_expiry
```

`mded` emits a warning toast when any cert is within 7 days of expiry.
Rotate proactively with `mackesd ca rotate`.

## Lighthouse administration

### Promote a peer to lighthouse

```bash
# Via GUI: Workbench → Network → Mesh → peer row → Promote to lighthouse
# Via CLI (on the lighthouse):
mackesd ca sign <node-id> --groups lighthouse,peer
```

Then restart `nebula.service` on the promoted peer:

```bash
ssh <peer> systemctl restart nebula.service
```

### Demote a lighthouse

1. Sign a new cert for the peer without the `lighthouse` group:
   `mackesd ca sign <node-id> --groups peer`
2. Remove the peer from the lighthouse hosts list in the Nebula config:
   Workbench → Network → Mesh → Lighthouses → remove the peer.
3. `nebula_supervisor` propagates the updated config to every peer
   within one heartbeat cycle.

## What if the CA host goes offline?

The CA private key is sealed on the lighthouse. If the lighthouse goes
permanently offline:

- Existing peers keep their certs and overlay connectivity until expiry.
- **No new peers can join** until a new CA is minted (full mesh-init).

To avoid this: keep a CA backup (see
[mesh-recovery.md](mesh-recovery.md)) and run multiple lighthouses so
at least one remains reachable.

## Audit log

Every CA operation lands in the event log:

```bash
mackesd logs --kind=audit         # tail live
mackesd events --kind=nebula_ca_rotated --since '2026-01-01'
mackesd audit verify              # verify the hash chain
```

`audit verify` walks the chain and reports the first row whose
`prev_hash` doesn't match. A failed verify is a serious finding —
do not trust audit data past the break point.
