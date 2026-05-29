#!/usr/bin/env bash
# Install the Mackes Recovery path:
#   1. systemd target unit  -> /etc/systemd/system/mackes-recovery.target
#   2. GRUB submenu entry   -> /etc/grub.d/40_mackes_recovery
#   3. mackes-recover CLI wrapper -> /usr/local/bin/mackes-recover
#
# Idempotent. Re-running updates the files in place.
set -euo pipefail

SHIP_DIR="${MACKES_SHELL_SHARE:-/usr/share/mde}"

if [[ $EUID -ne 0 ]]; then
    echo "install-recovery: must run as root (sudo $0)" >&2
    exit 2
fi

# 1. systemd target
install -m 0644 "$SHIP_DIR/data/systemd/mackes-recovery.target" \
    /etc/systemd/system/mackes-recovery.target
systemctl daemon-reload
echo "install-recovery: systemd target installed"

# 2. GRUB submenu entry
install -m 0755 "$SHIP_DIR/data/grub/40_mackes_recovery" \
    /etc/grub.d/40_mackes_recovery
if command -v grub2-mkconfig >/dev/null 2>&1; then
    GRUB_CFG="/boot/grub2/grub.cfg"
    [[ -d /boot/efi ]] && GRUB_CFG="/boot/efi/EFI/fedora/grub.cfg"
    grub2-mkconfig -o "$GRUB_CFG" >/dev/null
    echo "install-recovery: grub regenerated at $GRUB_CFG"
else
    echo "install-recovery: grub2-mkconfig not found; re-run after installing grub2-tools" >&2
fi

# 3. mackes-recover wrapper
cat > /usr/local/bin/mackes-recover <<'EOF'
#!/usr/bin/env bash
exec python3 -m mackes.recover "$@"
EOF
chmod 0755 /usr/local/bin/mackes-recover
echo "install-recovery: /usr/local/bin/mackes-recover installed"

echo "install-recovery: done. Reboot and select 'Mackes Recovery' from GRUB to test."
