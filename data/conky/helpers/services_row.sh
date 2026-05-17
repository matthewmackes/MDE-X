#!/bin/sh
# Conky helper — single-row "Services" summary.
# Merges what were three sections (Notifications / Media / Remote) into
# one compact line of three chip-style counts. Output format:
#
#   ✉ <notif>   ♪ <media>   ⌨ <remote>
#
# Each count is computed cheaply; failures degrade to "0".

# --- Notifications (mesh + local buckets) -----------------------------------
NOTIF_BUCKET="$HOME/QNM-Shared/.qnm-sync/notifications"
LOCAL_BUCKET="$HOME/QNM-Notifications/mine"
NOTIF=0
if [ -d "$NOTIF_BUCKET" ]; then
    M=$(find "$NOTIF_BUCKET" -type f 2>/dev/null | wc -l)
    NOTIF=$((NOTIF + M))
fi
if [ -d "$LOCAL_BUCKET" ]; then
    L=$(find "$LOCAL_BUCKET" -type f 2>/dev/null | wc -l)
    NOTIF=$((NOTIF + L))
fi

# --- Media services discovered on the mesh ----------------------------------
MEDIA=$(python3 - <<'PY' 2>/dev/null || echo 0
try:
    from mackes.mesh_services import load_registry
    print(len(load_registry()))
except Exception:
    print(0)
PY
)
[ -z "$MEDIA" ] && MEDIA=0

# --- Remote desktop active sessions (RDP + VNC + Guacamole) -----------------
RDP=$(ss -tn state established '( sport = :3389 )' 2>/dev/null | tail -n +2 | wc -l)
VNC=$(ss -tn state established '( sport = :5900 )' 2>/dev/null | tail -n +2 | wc -l)
GUAC=0
if [ -f /etc/guacamole/noauth-config.xml ]; then
    GUAC=$(grep -c '<connection ' /etc/guacamole/noauth-config.xml 2>/dev/null || echo 0)
fi
REMOTE=$((RDP + VNC + GUAC))

# Nerd Font glyphs:
#   ✉  bell        U+F0F3   \xef\x83\xb3
#   ♪  music       U+F001   \xef\x80\x81
#   ⌨  terminal    U+F120   \xef\x84\xa0
# Emit them as printf escape sequences — conky renders them via the
# current font, which is Hack Nerd Font in the template.
printf '\xef\x83\xb3 %s   \xef\x80\x81 %s   \xef\x84\xa0 %s\n' \
       "$NOTIF" "$MEDIA" "$REMOTE"
