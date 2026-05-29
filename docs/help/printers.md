# Printers — auto mesh sharing & sync

Any printer you add on one MDE peer shows up — and works — on every
other peer in your workgroup, with no setup. A background service
(`cups_sync`) keeps the fleet's printer list in sync over your Nebula
mesh; you never configure the same printer twice.

## How it works

- **Add a printer once.** Configure a printer on any peer the normal way
  (CUPS web UI at `http://localhost:631`, GNOME/KDE settings, or
  `lpadmin`). Within a few seconds it appears on every other peer.
- **All your printers are shared.** Every printer on every peer is
  shared to the whole mesh automatically — the same flat-trust model as
  the rest of your workgroup (one mesh, one passcode, everything
  shared). There's no per-printer "share" switch to remember.
- **Printing routes to the host.** When you print to a printer attached
  to another peer, the job travels your encrypted Nebula mesh to that
  peer, which sends it to the hardware. A USB printer plugged into your
  desktop is usable from your laptop across the house — or across the
  internet.

## Reading the printer list

In **Workbench → Printers**, remote printers are labelled with the peer
that hosts them:

```
Office          · this peer
Lab @forge      · on forge (online)
BigColor @nas   · on nas (offline)
```

- `· this peer` — a printer physically attached here.
- `· on <peer> (online)` — a remote printer whose host is up and
  reachable; print to it normally.
- `· on <peer> (offline)` — the host peer is asleep or off the mesh.
  You can still queue a job; it prints once that peer comes back. (A
  physically-attached printer can only print when its host is awake.)

Remote printers are named `<printer>@<host>` so two peers that both
have a printer called "Office" never collide — you'll see `Office@anvil`
and `Office@forge` as distinct queues.

## Defaults & options

Your default printer and saved print options (paper size, duplex, …)
sync across the fleet too, so picking a default on one peer carries to
the others. If two peers set a different default at nearly the same
time, the most recent one wins.

## Drivers

Modern printers use **driverless IPP Everywhere** — no driver to install
anywhere. For older printers, the host peer's driver definition (PPD)
is shared along with the printer, so other peers show the right options
without hunting for a driver. (Very old printers that need a proprietary
binary driver aren't supported over the mesh.)

## Which peers share printers

- **Workstation** and **headless** peers run print sharing — so a
  headless NAS with a USB printer attached can host it for everyone.
- **Lighthouse** peers (routing-only nodes) don't — they have no
  printers and stay lean.

## Troubleshooting

- **A printer isn't appearing on another peer.** Make sure both peers
  are enrolled in the mesh (`mde-update` shows the fleet) and `cups` is
  running (`systemctl status cups`). The sync runs every few seconds.
- **A remote printer shows "offline".** Its host peer is unreachable —
  check that peer is powered on and on the mesh.
- **Force a re-sync.** Re-opening the Printers panel reloads the list.
