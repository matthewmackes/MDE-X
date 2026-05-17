#!/bin/sh
# Conky helper — emit just the Nerd Font lock/unlock glyph.
# U+F09C unlock (sudoers NOPASSWD active OR cached sudo)
# U+F023 lock   (everything else)
if sudo -n true 2>/dev/null; then
    printf '\xef\x82\x9c'    #
else
    printf '\xef\x80\xa3'    #
fi
