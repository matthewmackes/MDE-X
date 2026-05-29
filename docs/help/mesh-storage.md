# Mesh Storage

`mesh-storage` is the shared, replicated filesystem behind your
peer's `~/Documents`, `~/Pictures`, `~/Music`, `~/Videos`, and
`~/Downloads` folders. Files you save there appear on every
other peer in the mesh. Files saved on any other peer appear on
yours. There is no upload step; the mesh handles it.

This page covers what is stored, where to put files you do not
want shared, how conflicts are resolved, and how to fix the
common questions that come up.

## What mesh-storage is

* **One export per fleet.** Every Mackes peer mounts the same
  LizardFS `mesh-storage` export. `mackesd` joins new peers
  automatically on enrolment and removes them when their
  certificate is revoked.
* **Replicated everywhere.** Every peer holds a full copy of
  every file. Goal = N is the rule: N enrolled peers means N
  copies, so pulling one peer's plug leaves all your data
  intact on the rest.
* **Active master with automatic failover.** LizardFS uses a
  metadata master. `mackesd` floats the master across your
  lighthouse peers behind a stable VIP address so clients
  reconnect transparently when a master peer goes offline.
* **Mounted in-place.** The five XDG dirs (`~/Documents`,
  `~/Pictures`, `~/Music`, `~/Videos`, `~/Downloads`) are
  mountpoints, not ordinary directories. Your file manager
  works the same way; the bytes live on the mesh.
* **Local-only escape hatch.** `~/Local/` is never shared. Use
  it for one-off files you do not want replicated.
* **Transport.** Everything moves over the Nebula overlay
  (`10.42.0.0/16`). Nothing crosses the public network in the
  clear.

## Where files live

| Folder | Shared? | Notes |
|--------|---------|-------|
| `~/Documents` | Yes | Replicated across every peer |
| `~/Pictures`  | Yes | Replicated across every peer |
| `~/Music`     | Yes | Replicated across every peer |
| `~/Videos`    | Yes | Replicated across every peer |
| `~/Downloads` | Yes | Replicated across every peer |
| `~/Local/`    | **No** | Local-only escape hatch |
| Anything else under `~` | **No** | `.bashrc`, `.config/`, app caches — local-only |

## Conflicts

Two peers can edit the same file while offline. When they
both reconnect, the mesh keeps the version with the latest
modification time (last-write-wins) and renames the loser to
`<filename>.conflict-<hostname>-<ts>.<ext>` in the same
folder.

The file browser shows a yellow chip on the conflict file.
Right-click → **Resolve…** opens a two-pane diff so you can
pick the winner. The loser moves to
`~/Local/conflict-archive/<ts>/` for safe-keeping.

If you do not resolve, both versions stay — the mesh never
silently drops your data.

## Trash and Undelete

Deleted files go into LizardFS trash and stay recoverable for
48 hours (configurable). Open Workbench → Mesh → Mesh Storage
and choose **Undelete recent** to browse and restore.

After the retention window the files are gone permanently.

## Disk space and quota

`mackesd` enforces a fleet-wide quota of **80% of the
smallest peer's free disk**. The Workbench → Mesh Storage
panel shows the current cap and which peer is the bottleneck.

When the cap is hit, every peer returns a "no space left"
error on new writes. The file browser surfaces a banner:
**Mesh almost full — peer-X has Y MB free**. Free space on the
constrained peer to lift the cap.

## Migrating existing files

When you first install v5.0.0 on a peer with pre-existing
`~/Documents` and similar directories, the birthright pipeline:

1. Moves the existing content to `~/Local/pre-mesh-<ts>/`
   (so nothing is lost).
2. Mounts `mesh-storage` on top of the XDG dirs.
3. Copies the archived content back into the mesh
   (`rsync --ignore-existing` — anything that already
   exists on another peer stays; your local file gets a
   `.conflict-<host>-<ts>` sibling instead).

You can review what was archived under `~/Local/pre-mesh-*/`
at any time and move content into the mesh manually if you
prefer.

## Paired phones

KDE Connect's send-file option drops files into
`~/Documents/From-<your-phone-name>/`. From there the mesh
replicates to every peer. There is no separate phone share
folder.

## Workbench → Mesh Storage panel

Workbench → Mesh → **Mesh Storage** gives you:

* **Export overview.** Total size, used, free, peer count,
  heal queue depth, conflict count.
* **Per-peer table.** Hostname, role (active master vs
  shadow), free disk, last seen, heal state.
* **Conflict list.** Every `.conflict-*` file currently in
  the export + a Resolve button per row.
* **Quota gauge.** Goes red at 80% utilisation.

The mesh-status applet in the bottom panel shows an at-a-glance
status: mesh in sync, heal pending N files, or offline.

## Common questions

**My phone uploaded a file and nothing happened on my laptop.**
The mesh-storage export only mounts after `mackesd.service` is
running and the peer has finished enrolling. Open Workbench
→ Mesh → Mesh Storage to confirm the export is mounted; if
not, the panel surfaces the reason. Phone files land first
in `~/Documents/From-<phone-name>/` on the receiving peer
before they replicate.

**I want to keep some files private to this peer.** Put them
under `~/Local/`. Nothing under `~/Local/` is ever shared.

**A peer is offline; can I still work?** Yes. Every peer
holds a full local copy. Edit files normally. When the peer
reconnects, changes replicate; any conflicts get the
`.conflict-*` treatment.

**Can I disable mesh storage on one peer?** Not yet — every
enrolled peer joins the mesh-storage export. To exclude a
peer, remove its node from the mesh entirely (`mackesd ca
revoke <node-id>`), which auto-shrinks the replication goal.
A future release may add per-peer opt-out without full
revocation.

## Related

* [Mesh Admin](mesh-admin.md) — node enrolment, CA, revocation.
* [Mesh SSH](mesh-ssh.md) — per-peer SSH config that's also
  built on the Nebula overlay.
* [Mesh Recovery](mesh-recovery.md) — restoring a bare peer
  from a `mackesd state backup` bundle (covers Nebula CA +
  LizardFS metadata dump + export config).
