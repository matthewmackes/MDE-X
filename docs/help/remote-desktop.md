# Remote Desktop

Every Mackes peer hosts three remote-desktop server paths,
all bound to the Nebula overlay only:

| Protocol | Daemon | Port | Default client | Auth model |
|----------|--------|------|----------------|------------|
| VNC | wayvnc | 5900 | remmina | Nebula overlay trust (Ed25519 once RD-4 lands) |
| RDP | xrdp | 3389 | remmina / Microsoft RDP client | xrdp's PAM + Xorg-fallback session |
| Web | Guacamole (via tomcat + caddy) | 8080 / via `https://media.mesh/desktop/` | any browser | noauth — mesh-firewall trust only |

Pick whichever fits your client. All three see the same peer
desktop; the choice is just about which client app you have
handy.

## VNC (wayvnc) — recommended on Wayland sessions

`mde-wayvnc@<your-user>.service` runs per-peer, attaches to
the live sway compositor via the wlroots screencopy protocol,
and binds VNC port 5900 to your peer's Nebula overlay IP
(read from `/var/lib/mackesd/nebula/overlay-ip`, published by
`mackesd`'s nebula supervisor).

**Connect from another peer:**

```
remmina -c vnc://<peer-name>.mesh:5900
```

(`.mesh` resolves to the overlay IP via the mDNS bridge or
your peer's `/etc/hosts` entries — see `mesh-services.md` for
the resolution chain.)

**v2.6 status:** wayvnc runs `--unauthenticated`; the Nebula
overlay membership IS the trust boundary. Anyone who can reach
your peer's overlay IP can connect. A future v2.6.x update
(worklist RD-4) adds per-peer Ed25519 authentication on top.

**Why not x11vnc?** x11vnc mirrors an X11 `:0` display.
v2.0.0's hard-switch to sway (Wayland-only) removed `:0`, so
x11vnc's unit silently failed to bind. wayvnc is the sway-
native replacement; see `docs/design/v2.6-wayland-vnc.md` for
the 5-Q lock rationale.

## RDP (xrdp)

`xrdp.service` is the SysV-style RDP server. Unlike wayvnc,
xrdp brings up its own Xorg fallback session — if your peer
runs a Wayland greeter, the RDP connection lands in an Xorg
session inside the xrdp process rather than your live sway
compositor.

**Connect from another peer:**

```
remmina -c rdp://<peer-name>.mesh:3389
```

Or from Windows: paste `<peer-name>.mesh` into the Microsoft
RDP client.

**Auth:** xrdp's stock PAM stack — log in with your peer's
local user/password.

## Guacamole (web)

`tomcat.service` hosts the Apache Guacamole web app at
`https://media.mesh/desktop/`. It speaks both VNC and RDP to
the local stack via `guacd.service`, so you can connect to
your peer from any browser without installing a native
client.

**Connection list:** auto-generated from the Nebula peer
roster by `mackes-remote-sync.service`. Every enrolled peer
appears as a pre-configured connection.

**Auth:** noauth — the Nebula overlay membership + Guacamole's
private CA trust the request. No login screen.

## Firewall

The birthright step (`apply_remote_desktop`) opens
`3389/tcp`, `5900/tcp`, and `8080/tcp` on the
`firewalld` `trusted` zone only. The mesh interface lives in
the trusted zone; the public underlay does not. An attacker on
the public network cannot reach any of the three ports.

## Common questions

**Why does `vnc://<peer>.mesh:5900` get connection-refused
right after installation?** wayvnc's unit gates on
`/var/lib/mackesd/nebula/overlay-ip` existing. That file gets
written by `mackesd`'s nebula supervisor after first peer
enrollment. Wait for `systemctl is-active mackesd.service` +
the enrollment ack, then `systemctl --user start
mde-wayvnc@<your-user>.service` retries.

**Can I connect to my peer from outside the mesh?** No. All
three servers bind to the Nebula overlay address only. From
outside the mesh, you'd see no listener on the public
interface. This is intentional per the open-mesh / flat-trust
directive — the mesh boundary IS the security boundary.

**Why is xrdp PAM-authenticated but VNC and Guacamole are
not?** xrdp brings its own Xorg session and the operator
expects a login prompt. VNC mirrors the active desktop —
the operator is already logged in there. Guacamole's noauth
mode mirrors the legacy v1.x setup that the mesh-firewall
trust replaces.

## Related

* [Network](network.md) — Wi-Fi, VPN, basic networking.
* [Mesh Services](mesh-services.md) — the catalog + how media
  services advertise.
* [Mesh SSH](mesh-ssh.md) — overlay-bound SSH that mirrors
  the same trust model.
* [Mesh Admin](mesh-admin.md) — enrollment, CA, peer
  lifecycle.
