/*
 * mackes-drawer — Mackes Notification Drawer xfce4-panel plugin (external).
 *
 * Replaces:
 *   * Conky HUD (mackes/conky_hud.py)
 *   * Mackes tray icon (mackes/tray.py)
 *   * Mini slide-in popover (mackes.app --popover)
 *
 * The plugin renders a single pill on the panel:
 *
 *   [ █ Mackes  Mon May 18  10:34  •  ◐ 3  •  ⚡ 77%  ⌃ ]
 *
 * Reads its display state from ~/.cache/mackes/drawer-state.json (written
 * by the running mackes-shell daemon). Refreshes every 5 seconds. On click,
 * spawns `mackes-shell --drawer` which opens the slide-in drawer window
 * (mackes.drawer module).
 *
 * Compiled to /usr/lib/xfce4/panel/plugins/mackes-drawer.
 * Registered via /usr/share/xfce4/panel/plugins/mackes-drawer.desktop.
 *
 * Design source: docs/design/v2.2.0-notification-drawer/
 *   "Mackes Notification Drawer.html"
 *
 * GPL-3.0, ©2026 Matthew Mackes.
 */

#include <gtk/gtk.h>
#include <gtk/gtkx.h>          /* GtkPlug — split out of gtk/gtk.h in GTK3 */
#include <X11/Xlib.h>          /* Window typedef */
#include <glib.h>
#include <glib/gstdio.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>


#define REFRESH_INTERVAL_MS 5000


typedef struct {
    GtkWidget *button;        /* the panel pill — clickable */
    GtkWidget *grid;          /* horizontal box holding all sub-labels */
    GtkWidget *brand_lbl;     /* "Mackes" or glyph */
    GtkWidget *time_lbl;      /* clock */
    GtkWidget *date_lbl;      /* date */
    GtkWidget *notif_box;     /* contains glyph + count */
    GtkWidget *notif_count;   /* notification count badge */
    GtkWidget *battery_lbl;   /* battery % */
    GtkWidget *caret_lbl;     /* ▾ when closed, ▴ when open */
    gchar     *state_path;    /* ~/.cache/mackes/drawer-state.json */
    guint      timer_id;      /* g_timeout_add return for refresh */
    gint64     state_mtime;   /* last seen mtime, only reparse on change */
} MackesDrawer;


/* ------------------------------------------------------------- state file */


/* Very small JSON-extract: find the first occurrence of `"key": <value>`
 * and return the value as a heap-allocated string. Sufficient for the
 * flat state file we generate. Caller frees. Returns NULL on miss. */
static gchar *
json_extract (const gchar *body, const gchar *key)
{
    if (!body || !key) return NULL;
    gchar *needle = g_strdup_printf ("\"%s\"", key);
    const gchar *p = strstr (body, needle);
    g_free (needle);
    if (!p) return NULL;
    p = strchr (p, ':');
    if (!p) return NULL;
    p++;
    while (*p && (*p == ' ' || *p == '\t')) p++;
    if (*p == '"') {
        p++;
        const gchar *end = strchr (p, '"');
        if (!end) return NULL;
        return g_strndup (p, (gsize) (end - p));
    }
    /* number or bool */
    const gchar *end = p;
    while (*end && *end != ',' && *end != '}' && *end != '\n' && *end != ' ')
        end++;
    return g_strndup (p, (gsize) (end - p));
}


static gint64
file_mtime (const gchar *path)
{
    GStatBuf st;
    if (g_stat (path, &st) != 0) return 0;
    return (gint64) st.st_mtime;
}


static void
apply_state_from_file (MackesDrawer *self)
{
    gint64 mt = file_mtime (self->state_path);
    if (mt != 0 && mt == self->state_mtime) {
        /* unchanged — still need to refresh the local clock so the time
         * stays accurate without a re-read */
        time_t now = time (NULL);
        struct tm tm_buf;
        localtime_r (&now, &tm_buf);
        gchar tbuf[16], dbuf[24];
        strftime (tbuf, sizeof tbuf, "%H:%M", &tm_buf);
        strftime (dbuf, sizeof dbuf, "%a %b %e", &tm_buf);
        gtk_label_set_text (GTK_LABEL (self->time_lbl), tbuf);
        gtk_label_set_text (GTK_LABEL (self->date_lbl), dbuf);
        return;
    }
    self->state_mtime = mt;

    /* Read full file */
    gchar *body = NULL;
    gsize blen = 0;
    if (!g_file_get_contents (self->state_path, &body, &blen, NULL)) {
        /* No state file yet — show defaults derived from local clock */
        time_t now = time (NULL);
        struct tm tm_buf;
        localtime_r (&now, &tm_buf);
        gchar tbuf[16], dbuf[24];
        strftime (tbuf, sizeof tbuf, "%H:%M", &tm_buf);
        strftime (dbuf, sizeof dbuf, "%a %b %e", &tm_buf);
        gtk_label_set_text (GTK_LABEL (self->time_lbl), tbuf);
        gtk_label_set_text (GTK_LABEL (self->date_lbl), dbuf);
        gtk_label_set_text (GTK_LABEL (self->notif_count), "0");
        gtk_label_set_markup (GTK_LABEL (self->battery_lbl),
                              "<span color=\"#a8a8a8\">⚡</span>");
        return;
    }

    gchar *time_str = json_extract (body, "time");
    gchar *date_str = json_extract (body, "date");
    gchar *notif    = json_extract (body, "notif_count");
    gchar *batt     = json_extract (body, "battery_pct");

    if (time_str)
        gtk_label_set_text (GTK_LABEL (self->time_lbl), time_str);
    else {
        time_t now = time (NULL);
        struct tm tm_buf;
        localtime_r (&now, &tm_buf);
        gchar tbuf[16];
        strftime (tbuf, sizeof tbuf, "%H:%M", &tm_buf);
        gtk_label_set_text (GTK_LABEL (self->time_lbl), tbuf);
    }
    if (date_str)
        gtk_label_set_text (GTK_LABEL (self->date_lbl), date_str);
    else {
        time_t now = time (NULL);
        struct tm tm_buf;
        localtime_r (&now, &tm_buf);
        gchar dbuf[24];
        strftime (dbuf, sizeof dbuf, "%a %b %e", &tm_buf);
        gtk_label_set_text (GTK_LABEL (self->date_lbl), dbuf);
    }
    gtk_label_set_text (GTK_LABEL (self->notif_count),
                        notif && *notif ? notif : "0");
    if (batt && *batt) {
        gchar *m = g_strdup_printf ("<span color=\"#a8a8a8\">⚡</span> %s%%", batt);
        gtk_label_set_markup (GTK_LABEL (self->battery_lbl), m);
        g_free (m);
    } else {
        gtk_label_set_markup (GTK_LABEL (self->battery_lbl),
                              "<span color=\"#a8a8a8\">⚡</span>");
    }

    g_free (time_str);
    g_free (date_str);
    g_free (notif);
    g_free (batt);
    g_free (body);
}


static gboolean
refresh_tick (gpointer data)
{
    MackesDrawer *self = data;
    apply_state_from_file (self);
    return G_SOURCE_CONTINUE;
}


/* ----------------------------------------------------- click → open drawer */


static void
on_clicked (GtkButton *btn, gpointer data)
{
    MackesDrawer *self = data;
    /* Spawn mackes-shell --drawer. The Python side toggles the drawer
     * window so re-clicking the panel pill closes it. */
    const gchar *argv[] = { "mackes-shell", "--drawer", NULL };
    GError *err = NULL;
    if (!g_spawn_async (NULL, (gchar **) argv, NULL,
                        G_SPAWN_SEARCH_PATH,
                        NULL, NULL, NULL, &err)) {
        g_warning ("mackes-drawer: could not spawn mackes-shell --drawer: %s",
                   err ? err->message : "?");
        if (err) g_error_free (err);
    }
    /* Caret flips in apply_state_from_file once the drawer writes its
     * `open: true` flag into the state file. */
    (void) btn;
    (void) self;
}


/* ------------------------------------------------------- widget construction */


static void
build_pill (MackesDrawer *self)
{
    self->button = gtk_button_new ();
    gtk_button_set_relief (GTK_BUTTON (self->button), GTK_RELIEF_NONE);
    gtk_widget_set_tooltip_text (self->button,
        "Mackes Shell — click for status drawer");

    self->grid = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 6);

    /* Accent stripe */
    GtkWidget *stripe = gtk_label_new (" ");
    GtkStyleContext *sc = gtk_widget_get_style_context (stripe);
    gtk_style_context_add_class (sc, "mackes-drawer-stripe");
    GtkCssProvider *css = gtk_css_provider_new ();
    gtk_css_provider_load_from_data (css,
        ".mackes-drawer-stripe { background: #4589ff; "
        "min-width: 2px; padding: 0; margin: 0 2px 0 0; }", -1, NULL);
    gtk_style_context_add_provider (sc, GTK_STYLE_PROVIDER (css),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref (css);
    gtk_box_pack_start (GTK_BOX (self->grid), stripe, FALSE, FALSE, 0);

    /* Brand glyph */
    self->brand_lbl = gtk_label_new ("●");
    gtk_label_set_markup (GTK_LABEL (self->brand_lbl),
        "<span color=\"#4589ff\" weight=\"bold\">▤</span>");
    gtk_box_pack_start (GTK_BOX (self->grid), self->brand_lbl, FALSE, FALSE, 0);

    /* Date */
    self->date_lbl = gtk_label_new ("");
    gtk_label_set_use_markup (GTK_LABEL (self->date_lbl), TRUE);
    GtkStyleContext *dsc = gtk_widget_get_style_context (self->date_lbl);
    GtkCssProvider *dcss = gtk_css_provider_new ();
    gtk_css_provider_load_from_data (dcss,
        "label { color: #a8a8a8; font-size: 11px; }", -1, NULL);
    gtk_style_context_add_provider (dsc, GTK_STYLE_PROVIDER (dcss),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref (dcss);
    gtk_box_pack_start (GTK_BOX (self->grid), self->date_lbl, FALSE, FALSE, 0);

    /* Time */
    self->time_lbl = gtk_label_new ("--:--");
    GtkStyleContext *tsc = gtk_widget_get_style_context (self->time_lbl);
    GtkCssProvider *tcss = gtk_css_provider_new ();
    gtk_css_provider_load_from_data (tcss,
        "label { color: #f4f4f4; font-weight: 600; font-size: 13px; }",
        -1, NULL);
    gtk_style_context_add_provider (tsc, GTK_STYLE_PROVIDER (tcss),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref (tcss);
    gtk_box_pack_start (GTK_BOX (self->grid), self->time_lbl, FALSE, FALSE, 0);

    /* Separator */
    GtkWidget *sep1 = gtk_label_new ("·");
    gtk_widget_set_sensitive (sep1, FALSE);
    gtk_box_pack_start (GTK_BOX (self->grid), sep1, FALSE, FALSE, 0);

    /* Notification glyph + count */
    self->notif_box = gtk_box_new (GTK_ORIENTATION_HORIZONTAL, 4);
    GtkWidget *bell = gtk_label_new (NULL);
    gtk_label_set_markup (GTK_LABEL (bell), "<span color=\"#a8a8a8\">◐</span>");
    gtk_box_pack_start (GTK_BOX (self->notif_box), bell, FALSE, FALSE, 0);
    self->notif_count = gtk_label_new ("0");
    GtkStyleContext *nsc = gtk_widget_get_style_context (self->notif_count);
    GtkCssProvider *ncss = gtk_css_provider_new ();
    gtk_css_provider_load_from_data (ncss,
        "label { background: rgba(255,255,255,0.10); "
        "color: #f4f4f4; border-radius: 9px; "
        "padding: 0 6px; font-size: 10px; font-weight: 600; }", -1, NULL);
    gtk_style_context_add_provider (nsc, GTK_STYLE_PROVIDER (ncss),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref (ncss);
    gtk_box_pack_start (GTK_BOX (self->notif_box), self->notif_count,
                         FALSE, FALSE, 0);
    gtk_box_pack_start (GTK_BOX (self->grid), self->notif_box, FALSE, FALSE, 0);

    /* Separator */
    GtkWidget *sep2 = gtk_label_new ("·");
    gtk_widget_set_sensitive (sep2, FALSE);
    gtk_box_pack_start (GTK_BOX (self->grid), sep2, FALSE, FALSE, 0);

    /* Battery */
    self->battery_lbl = gtk_label_new (NULL);
    gtk_label_set_markup (GTK_LABEL (self->battery_lbl),
        "<span color=\"#a8a8a8\">⚡</span>");
    gtk_box_pack_start (GTK_BOX (self->grid), self->battery_lbl,
                         FALSE, FALSE, 0);

    /* Caret */
    self->caret_lbl = gtk_label_new ("▾");
    GtkStyleContext *csc = gtk_widget_get_style_context (self->caret_lbl);
    GtkCssProvider *ccss = gtk_css_provider_new ();
    gtk_css_provider_load_from_data (ccss,
        "label { color: #a8a8a8; font-size: 11px; padding-left: 4px; }",
        -1, NULL);
    gtk_style_context_add_provider (csc, GTK_STYLE_PROVIDER (ccss),
        GTK_STYLE_PROVIDER_PRIORITY_APPLICATION);
    g_object_unref (ccss);
    gtk_box_pack_start (GTK_BOX (self->grid), self->caret_lbl, FALSE, FALSE, 0);

    gtk_container_add (GTK_CONTAINER (self->button), self->grid);

    /* Wire click + initial paint */
    g_signal_connect (self->button, "clicked",
                       G_CALLBACK (on_clicked), self);
    apply_state_from_file (self);
}


/* ----------------------------------------------------- plugin entry point */


int main (int argc, char **argv)
{
    gtk_init (&argc, &argv);

    /* xfce4-panel passes the plugin socket id as argv[2] in the standard
     * external-plugin protocol. argv[1] is the plugin id (numeric).
     * If we're invoked standalone (no socket), fall back to a normal
     * top-level window — useful for `mackes-drawer --standalone` testing. */
    MackesDrawer self = { 0 };
    self.state_path = g_build_filename (g_get_user_cache_dir (),
                                          "mackes", "drawer-state.json",
                                          NULL);

    GtkWidget *toplevel;
    if (argc >= 3) {
        Window socket_id = (Window) g_ascii_strtoull (argv[2], NULL, 0);
        toplevel = gtk_plug_new (socket_id);
    } else {
        toplevel = gtk_window_new (GTK_WINDOW_TOPLEVEL);
        gtk_window_set_title (GTK_WINDOW (toplevel), "Mackes Drawer (preview)");
    }

    build_pill (&self);
    gtk_container_add (GTK_CONTAINER (toplevel), self.button);

    g_signal_connect (toplevel, "destroy",
                       G_CALLBACK (gtk_main_quit), NULL);

    self.timer_id = g_timeout_add (REFRESH_INTERVAL_MS, refresh_tick, &self);

    gtk_widget_show_all (toplevel);
    gtk_main ();

    if (self.timer_id) g_source_remove (self.timer_id);
    g_free (self.state_path);
    return 0;
}
