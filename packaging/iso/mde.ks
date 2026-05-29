# CB-4.2 — Mackes Desktop Environment (MDE) 2.0.0 kickstart for a
# custom Fedora live ISO. Replaces the v1.x mackes-xfce.ks.
#
# Build (on a Fedora host):
#
#     sudo dnf install -y lorax pykickstart
#     sudo livemedia-creator \
#         --make-iso \
#         --ks ./packaging/iso/mde.ks \
#         --no-virt \
#         --resultdir ./dist/iso \
#         --project "Mackes Desktop Environment" \
#         --releasever "$(rpm -E %fedora)" \
#         --volid "MDE"
#
# Result lands in ./dist/iso/<…>.iso. ~20-30 min on modern hardware.
#
# The OEM-mode wizard is wired via the post-install %post section,
# which drops a /etc/skel/.config/mde/state.json marking the
# install as un-provisioned so the wizard runs on first login.

lang en_US.UTF-8
keyboard us
timezone --utc Etc/UTC
selinux --enforcing
firewall --enabled --service=mdns
services --enabled=NetworkManager,sshd,lightdm
rootpw --plaintext mde
auth --useshadow --passalgo=sha512
firstboot --disable

bootloader --location=mbr --append="rhgb quiet"
zerombr
clearpart --all --initlabel
part / --size=8192 --grow --asprimary --fstype=ext4

# ---- Base + Wayland --------------------------------------------------------
url --mirrorlist=https://mirrors.fedoraproject.org/mirrorlist?repo=fedora-$releasever&arch=$basearch

repo --name=updates --mirrorlist=https://mirrors.fedoraproject.org/mirrorlist?repo=updates-released-f$releasever&arch=$basearch

# MDE repo (gh-pages — see data/dnf/mackes-shell.repo)
repo --name=mde \
    --baseurl=https://matthewmackes.github.io/MAP2-RELEASES/fedora/$releasever/$basearch \
    --includepkgs=mde

%packages
@core
# @base-x stays for Xwayland-compatible apps that haven't ported to
# Wayland yet. NOT @xfce-desktop-environment — XFCE is gone (CB-3.3
# Conflicts: blocks it).
@base-x
# Wayland compositor + ergonomics
sway
swaylock
swayidle
swaybg
foot
bemenu
brightnessctl
pipewire
wireplumber
grim
slurp
kanshi
wl-clipboard
wlr-randr
# Greeter + display manager
lightdm
lightdm-gtk
# Networking
NetworkManager-wifi
NetworkManager-vpnc
NetworkManager-openvpn
firewalld
openssh-server
# Power + removable media
power-profiles-daemon
upower
udisks2
# Pkg + flatpak tooling
dnf-plugins-core
flatpak
# Optional file managers — default-tier per CB-3.4 comps
cosmic-files
yazi
# Typography — PatternFly 6 (Red Hat)
redhat-display-fonts
redhat-text-fonts
redhat-mono-fonts
# MDE itself — base substrate + Wayland desktop addon. mde-core is
# GUI-free; mde-desktop ships the mde-panel/mde-portal/mde-applet-*
# binaries and pulls the sway stack via Requires. The ISO is a
# full-desktop image, so it installs BOTH (base mde alone would
# yield a headless box).
mde-core
mde-desktop
%end

# ---- Post-install ----------------------------------------------------------
%post

# Make sure the MDE wizard runs on first login of any user. Path is
# the new ~/.config/mde/ (post Phase 0 path rename).
mkdir -p /etc/skel/.config/mde
cat > /etc/skel/.config/mde/state.json <<'EOF'
{
  "provisioned": false,
  "active_preset": null,
  "schema_version": 2
}
EOF

# CB-2.3 — LightDM default session = mde so newly created accounts
# land on the MDE Wayland session. configure-lightdm.sh writes the
# matching drop-in for the live-iso install; on a normal install
# the same script runs via the first-run wizard.
mkdir -p /etc/lightdm/lightdm.conf.d
cat > /etc/lightdm/lightdm.conf.d/50-mde.conf <<'EOF'
[Seat:*]
user-session=mde
EOF

# CB-3.4 — register the comps group so future installs can
# `dnf groupinstall mackes-desktop-environment`.
dnf groups mark install mackes-desktop-environment 2>/dev/null || :

# Add the MDE dnf repo for future upgrades.
if [ -x /usr/share/mackes-shell/install-helpers/add-mackes-repo.sh ]; then
    /usr/share/mackes-shell/install-helpers/add-mackes-repo.sh || true
fi

# Wire the recovery boot entry (idempotent on re-runs).
if [ -x /usr/share/mackes-shell/install-helpers/install-recovery.sh ]; then
    /usr/share/mackes-shell/install-helpers/install-recovery.sh || true
fi

# CB-4.3 — branding hook. Install the MDE wallpaper as the
# system-wide default backdrop (used by the LightDM greeter +
# first-run wizard).
if [ -f /usr/share/mackes-shell/branding/standard-wallpaper.png ]; then
    install -D -m 0644 /usr/share/mackes-shell/branding/standard-wallpaper.png \
        /usr/share/backgrounds/mde-default.png
fi

# CB-4.3 — Plymouth theme. Activate the MDE theme by default on
# the ISO (in-tree birthright step keeps it opt-in on upgrades so
# we don't rebuild initrd silently). The theme assets ship under
# /usr/share/plymouth/themes/mde/ once the designer signs them
# off; until then this hook is a no-op when the dir is missing.
if [ -d /usr/share/plymouth/themes/mde ]; then
    plymouth-set-default-theme -R mde || \
        echo "warning: plymouth-set-default-theme failed; theme remains default"
fi

%end
