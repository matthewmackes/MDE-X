#!/bin/sh
# Conky helper — Mackes-managed service health, compact dot grid.
# Output: a row of "● name" entries, color from the unit's state.
#
# Conky doesn't have inline-color via shell output, so this script
# emits one line per service prefixed with a status glyph:
#   ●  active
#   ○  inactive / not found
#   ⚠  activating
#   ✗  failed

dot_for() {
    case "$1" in
        active)     printf '●' ;;
        activating) printf '⚠' ;;
        failed)     printf '✗' ;;
        *)          printf '○' ;;
    esac
}

UNITS="sshd headscale tailscaled guacd tomcat mackes-remote-sync mackes-ansible-pull caddy"
LINE=""
for u in $UNITS; do
    STATE=$(systemctl is-active "$u" 2>/dev/null || echo unknown)
    DOT=$(dot_for "$STATE")
    # Print as two services per line for compactness
    LINE="$LINE  $DOT $u"
done

# Wrap at ~50 chars per line by splitting at " ● " / " ○ " boundaries
echo "$LINE" | fold -s -w 50 | sed 's/^ //'
