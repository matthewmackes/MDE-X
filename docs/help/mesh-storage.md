# Mesh Storage

`mesh-home` is the shared, replicated filesystem behind your
peer's `~/Documents`, `~/Pictures`, `~/Music`, `~/Videos`, and
`~/Downloads` folders. Files you save there appear on every
other peer in the mesh. Files saved on any other peer appear on
yours. There is no "upload" step; the mesh handles it.

This page covers what's stored, where to put files you DON'T
want shared, how conflicts are resolved, and how to fix the
common questions that come up.

## What `mesh-home` is

* **One volume per fleet.** Every Mackes peer mounts the same
  GlusterFS volume named `mesh-home`. Glue logic in `mackesd`
  joins new peers automatically on enrolment and removes them
  when their certificate is revoked.
* **Replicated everywhere.** Every peer holds the full copy of
  every file in `mesh-home`. There is no "primary" host. Pull
  one peer's plug and the rest carry on.
* **Mounted in-place.** The five XDG dirs (`~/Documents`,
  `~/Pictures`, `~/Music`, `~/Videos`, `~/Downloads`) are not
  local directories. They are mountpoints. Your file manager
  works the same way; the bytes live on the mesh.
* **Local-only escape hatch.** `~/Local/` is never shared. Use
  it for one-off files you don't want replicated.
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

## The 5 GB cap

Files larger than 5 GiB do NOT replicate in full. Instead, the
peer that originally wrote the file holds the bytes; every
other peer sees a `.mesh-stub` placeholder. Open
`crates/mde-files`'s file browser, right-click the stub, and
choose **Fetch from peer-X** to pull the bytes on demand.

This keeps the mesh from drowning a small peer's disk when
someone drops a 50 GB video into `~/Videos`. The default cap
is fleet-wide and configurable in a future release.

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

If you don't resolve, both versions stay — the mesh never
silently drops your data.

## Disk space

`mackesd` polls free space on every peer every hour and
enforces a fleet-wide quota of **80% of the smallest peer's
free brick**. The Workbench → Mesh Storage panel shows the
current cap + which peer is the bottleneck.

When the cap is hit, every peer returns `EROFS` on new
writes. The file browser surfaces a banner: **Mesh almost
full — peer-X has Y MB free**. Free space on the constrained
peer to lift the cap.

## Migrating existing files

When you first install the v5.0.0 mesh on a peer with pre-
existing `~/Documents` / etc., the birthright pipeline:

1. Moves the existing content to `~/Local/pre-mesh-<ts>/`
   (so nothing is lost).
2. Mounts `mesh-home` on top of the XDG dirs.
3. Copies the archived content back into the mesh
   (`rsync --ignore-existing` — anything that already
   exists in the mesh stays; your local file gets a
   `.conflict-<host>-<ts>` sibling instead).

You can review what was archived under `~/Local/pre-mesh-*/`
at any time and move content into the mesh manually if you
prefer.

## Paired phones

KDE Connect's "send file" option drops files into
`~/Documents/From-<your-phone-name>/`. From there the mesh
replicates to every peer. There is no separate phone share
folder.

The KDE Connect UI no longer offers a generic file-share
button — it just routes everything into the mesh drop folder.

## Workbench → Mesh Storage panel

Workbench → Mesh → **Mesh Storage** gives you:

* **Volume overview.** Total size, used, free, peer count,
  heal queue depth, conflict count.
* **Per-peer table.** Hostname, role (genesis vs joiner),
  free brick, last seen, heal state.
* **Conflict list.** Every `.conflict-*` file currently in
  the volume + a Resolve button per row.
* **Quota gauge.** Goes red at 80% utilisation.

The mesh-status applet in the bottom panel shows an at-a-glance
status: "mesh in sync", "heal pending N files", or "offline".

## Common questions

**My phone uploaded a file and nothing happened on my laptop.**
The mesh-home volume only mounts after `mackesd.service` is
running and the peer has finished enrolling. Open Workbench
→ Mesh → Mesh Storage to confirm the volume is mounted; if
not, the panel surfaces the reason. Phone files land first
in `~/Documents/From-<phone-name>/` on the receiving peer
before they replicate.

**I copied a 20 GB video and only see a tiny `.mesh-stub`
file on my other laptop.** That's the size-cap behaviour
working as designed. Right-click the stub in the file browser
and choose **Fetch from peer-X** to pull the real bytes.

**I want to keep some files private to this peer.** Put them
under `~/Local/`. Nothing under `~/Local/` is ever shared.

**A peer is offline; can I still work?** Yes. Every peer
holds a full local copy. Edit files normally. When the peer
reconnects, changes replicate; any conflicts get the
`.conflict-*` treatment.

**Can I disable mesh storage on one peer?** Not yet — every
enrolled peer joins the mesh-home volume. To exclude a peer,
remove its node from the mesh entirely (`mackesd ca revoke
<node-id>`), which auto-shrinks the volume's replica count.
A future release may add per-peer opt-out without full
revocation.

## Related

* [Mesh Admin](mesh-admin.md) — node enrolment, CA, revocation.
* [Mesh SSH](mesh-ssh.md) — per-peer SSH config that's also
  built on the Nebula overlay.
* [Mesh Recovery](mesh-recovery.md) — restoring a bare peer
  from a `mackesd state backup` bundle (covers Nebula CA +
  Gluster volume topology).
