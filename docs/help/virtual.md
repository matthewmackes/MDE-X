# Virtual

`mde-virtual` is the mesh-aware compute manager — a single app for the
KVM virtual machines and Podman containers running across your whole
workgroup. Launch it from the Workbench **Virtual** tile, the Dock, or
Portal search (type "Virtual").

The window has two tabs: **Fleet** (every peer's compute, read-only for
other peers) and **Local** (this machine's compute, with full controls).

## Fleet tab

One collapsible section per peer (its hostname and Nebula IP), each
listing that peer's VMs and containers. Every row shows the name, a type
badge (**KVM** or **Podman**), a state badge (running / paused /
stopped), CPU %, RAM, and — for VMs — the Nebula IP (containers show
their image instead).

Fleet rows are read-only: you watch the fleet here, but you operate each
machine from its owner's Local tab (or by acting on a remote VM's detail
panel, which stays read-only). If the mesh is unreachable, the Fleet tab
shows a **Mesh unavailable** banner; the Local tab keeps working.

The data comes from each peer's `mded`, which publishes its compute
inventory to the mesh every few seconds, so the Fleet view is the union
across all peers.

## Local tab

Your own VMs and containers, with controls enabled. Each VM's quick
actions sit on its row; click a VM's name to open its **detail panel**.
When the mesh is down, the Local tab falls back to reading libvirt and
`podman` directly, so you never lose control of your own machines.

## Creating a VM

Click **+ New VM** (top right of the Local tab) for the four-step wizard:

1. **Name** — a short name (letters, digits, hyphens); a unique suffix is
   appended automatically. Optionally start from a saved **template**.
2. **CPU & RAM** — vCPUs (1–16) and RAM in MB (512–65536); defaults are
   2 vCPU / 2048 MB.
3. **Disk & ISO** — disk size in GB (10–500), an installer ISO (pick from
   `/var/lib/mde-vms/isos/` or type a path), and **Share MeshFS** (on by
   default — see below).
4. **Review & create** — confirm the summary and **Create**.

A status banner tracks provisioning (**Creating… → Created** with the
VM's name and Nebula IP, or **Create failed** with the reason). Cancel
from any step; nothing is created until you press Create.

## The VM detail panel

Click a VM's name to open its panel. It shows the state, CPU % and RAM
(with live sparklines that trend the last couple of minutes for local
VMs), the disk path, the Nebula IP, and a **MeshFS** badge. For local
VMs the panel offers:

- **Lifecycle** — Start, Stop, Force off, Suspend, Resume (only the
  actions valid for the current state are enabled).
- **Console** — opens the graphical console with `virt-viewer`.
- **Snapshots** — list the VM's libvirt snapshots, take a new one, or
  delete one.
- **Exposed ports** — see which guest ports are forwarded and on which
  networks; **Expose port…** opens a form (guest port, TCP/UDP, and the
  networks to expose on — Mesh, LAN, WAN); remove a forward with its `×`.
- **Migrate to…** — pick another online peer to move the VM to.
- **Save as template…** — capture the VM's vCPU/RAM/disk/MeshFS settings
  as a reusable template.

## MeshFS access

When **Share MeshFS** is on, the VM mounts the mesh-storage filesystem
over virtiofs, so the guest sees the same shared files as the host. Turn
it off for a fully isolated VM.

## Exposing a port

A VM's services are reachable only on the Nebula overlay by default. Use
**Expose port…** to forward a guest port onto one or more networks:

- **Mesh** — reachable by other peers over the overlay.
- **LAN** — reachable on the local network.
- **WAN** — reachable from the wider internet (use sparingly).

## Cold migration

**Migrate to…** moves a VM to another peer: the source shuts it down,
copies its disk over the Nebula overlay, and the target brings it back
up. The VM keeps its Nebula identity, so its mesh address follows it.

## Templates

Saved templates live on mesh-storage, so a template you save on one peer
is available in the wizard on every peer. Manage them from the wizard's
step 1 (apply or delete) and create them with **Save as template…** in a
VM's detail panel.

## Containers

Podman containers appear alongside VMs with a **Podman** badge and their
image. Local containers support Start and Stop from their row.
