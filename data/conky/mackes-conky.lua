-- mackes-conky.lua — left-edge accent stripe.
--
-- Replaces the per-line ${color}┃${color} BAR prefix with a single
-- cairo-drawn 3px stripe down the left edge of the HUD window. Two
-- wins: less horizontal real-estate (~24px reclaimed across the
-- column), and one cairo stroke instead of N text-substitution +
-- font-switch passes per refresh.
--
-- The accent hex is substituted at config-render time by
-- conky_hud.render_config — see mackes/conky_hud.py.

-- Conky ships its cairo + cairo_xlib Lua bindings under
-- /usr/lib64/conky/ (Fedora) or /usr/lib/conky/ (Debian). Lua's
-- default package.cpath doesn't search there, so extend it before
-- require()ing.
package.cpath = '/usr/lib64/conky/lib?.so;/usr/lib/conky/lib?.so;'
             .. package.cpath
require 'cairo'
-- cairo_xlib exposes cairo_xlib_surface_create separately from the
-- core cairo module (conky 1.18+). Best-effort require — fall back
-- to cairo's own export if cairo_xlib isn't built.
pcall(require, 'cairo_xlib')

local STRIPE_WIDTH = 3
local STRIPE_HEX   = '{accent_hex}'

local function hex_to_rgb(hex)
    local r = tonumber(hex:sub(1,2), 16) / 255
    local g = tonumber(hex:sub(3,4), 16) / 255
    local b = tonumber(hex:sub(5,6), 16) / 255
    return r, g, b
end

function conky_draw_stripe()
    if conky_window == nil then return end
    local cs = cairo_xlib_surface_create(
        conky_window.display, conky_window.drawable,
        conky_window.visual, conky_window.width, conky_window.height)
    local cr = cairo_create(cs)
    local r, g, b = hex_to_rgb(STRIPE_HEX)
    cairo_set_source_rgba(cr, r, g, b, 1.0)
    cairo_rectangle(cr, 0, 0, STRIPE_WIDTH, conky_window.height)
    cairo_fill(cr)
    cairo_destroy(cr)
    cairo_surface_destroy(cs)
end
