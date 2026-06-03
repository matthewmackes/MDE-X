# Music

The native music player for MDE. **`mde-music`** is the Iced/Rust front-end;
**`mde-musicd`** is the daemon that owns the playback engine, the MPRIS
surface, the play queue, and the mesh coordination. The two talk over the
Mackes Bus (`action/music/*` request → `reply/<ulid>`), so the GUI never
touches your music server directly.

The player is an Airsonic / Subsonic client — it plays from any
Subsonic-API server (Airsonic, Navidrome, Gonic, …) on your LAN or mesh.

## Connecting (first run)

On first launch `mde-music` shows a connect form: server URL, username,
password. Credentials are written to
`~/.local/share/mde/airsonic-creds.json` and the daemon reads them from
there. Re-run the connect form any time with `mde-music --first-run`.

The daemon reconnects on its own with a backoff schedule if the server
drops, so a server reboot doesn't need operator intervention.

## The library hub

The home view is a hub of category cards:

- **Albums** — every album, newest first.
- **Artists** — the artist index.
- **Genres** — your server's genres; open one for its albums.
- **Playlists** — your saved playlists; click one to play it.
- **Recents** — recently-added albums.
- **Podcasts** — subscribed channels; open one for its episodes.
- **Radio** — your server's internet-radio stations.

Open a category for a card grid. The grid:

- **Reflows its columns** to the window width — widen the window for more
  columns, narrow it for fewer.
- Has a **Sort** toggle (Name A–Z / Z–A); the choice persists across
  launches (`~/.local/share/mde/music-prefs.json`).
- Tracks a **breadcrumb** (`Library → … `) so you can jump back up the
  path; it caps at four visible segments and elides the middle when
  deeper.

## Playing

Open an album for the album page: cover art (the chrome tints to the
cover's dominant colour), then **Play** / **Shuffle** / **Add to Queue**
above a numbered track list. Each track row has a menu — **Play Next** and
**Add to Queue**. Clicking a **Playlist** plays the whole playlist
(replace the queue + start). Clicking a podcast **episode** plays it.

Playback is native and gapless: the daemon decodes with Symphonia
(FLAC / MP3 / Vorbis / AAC / Opus) and pre-buffers the next track during
the last seconds of the current one, so albums play with no gap between
tracks. Output goes through cpal (PipeWire on a standard MDE install).

A **now-playing footer** shows the current track + transport controls
(play / pause / next / previous) + a volume slider. Hardware media keys
and any MPRIS controller (lock screen, status bar) drive the same engine
through the daemon's `org.mpris.MediaPlayer2` surface.

## Search

The title-bar search box (focus with **Cmd/Super-F**) queries artists,
albums, and songs at once; **Esc** dismisses the results sheet. Add a
song to the queue straight from the results.

## On the mesh

`mde-music` is mesh-aware:

- **Shared cache.** Fetched + decoded media is cached on the mesh-storage
  volume and shared between peers, with LRU eviction when the cache cap is
  reached — so a track another peer already pulled doesn't get re-fetched.
- **One player at a time.** A mesh state file coordinates exclusive
  playback across peers: starting playback on one machine lets you **take
  over** from whichever peer was playing, rather than two machines playing
  over each other.

## In the Workbench

**Devices → Music** in the MDE Workbench is the settings/status panel for
the player + daemon.

## The daemon

`mde-musicd serve` runs the daemon (it is started as a user service on a
normal install). It owns the decode/output engine, the play queue, the
MPRIS interface, and the Bus responder. Browse and transport both flow
through the Bus: `action/music/{list-albums, list-artists, list-genres,
albums-by-genre, list-playlists, get-playlist, list-recents, list-podcasts,
podcast-episodes, get-album, get-song, search, get-cover-art}` for browse,
and `action/music/{play, pause, resume, stop, next, prev, set-volume,
enqueue, enqueue-after, clear, get-queue, get-state}` for control.
