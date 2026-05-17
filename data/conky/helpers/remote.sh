#!/bin/sh
# Conky helper — Remote desktop active session counts.

# xrdp counts an active session via xrdp-sesrun output, but the easiest
# proxy is the count of active RDP TCP connections in netstat / ss.
RDP=$(ss -tn state established '( sport = :3389 )' 2>/dev/null | tail -n +2 | wc -l)
VNC=$(ss -tn state established '( sport = :5900 )' 2>/dev/null | tail -n +2 | wc -l)
# Guacamole connection count — read the live noauth-config and count.
GUAC=0
if [ -f /etc/guacamole/noauth-config.xml ]; then
    GUAC=$(grep -c '<connection ' /etc/guacamole/noauth-config.xml 2>/dev/null || echo 0)
fi

printf '  RDP · %s  VNC · %s\n' "${RDP:-0}" "${VNC:-0}"
printf '  guacamole · %s connections\n' "${GUAC}"
