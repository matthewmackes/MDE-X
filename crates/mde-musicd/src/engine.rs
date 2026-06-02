//! AIR-5 (v6.1) — native gapless playback engine.
//!
//! The engine decodes a track's bytes with **Symphonia** (pure-Rust:
//! FLAC / MP3 / Vorbis / AAC / WAV) and plays them through **cpal**
//! (ALSA → PipeWire on this host). Tracks handed to [`Engine::play`] are
//! decoded back-to-back into one continuous sample ring, so album
//! playback is **gapless by construction** — the next track's samples
//! land immediately after the current track's, with no drain in between.
//!
//! Opus is decoded through libopus in AIR-5.b — Symphonia 0.5 ships no
//! Opus codec, so [`SourceCodec::Opus`] is reported as unsupported by
//! this build rather than mis-probed.
//!
//! Per §0.12 the engine is reachable from a runtime entry point
//! (`mde-musicd play <song-id>…`); per §0.15 the audible-output
//! acceptance (gap-free album playback) is a release HW-bench item. The
//! decode/output side effects therefore aren't unit-tested here — the
//! mechanically-checkable core (codec hinting, the gapless schedule, the
//! volume/resample/channel-map math, the underrun-fill contract) is, and
//! is the same code the side-effecting paths drive.

// Pure DSP / doc style lints that are noise for an audio module: the
// resampler + channel mapper do intentional, bounded integer↔float
// casts; product names in prose (PipeWire / ALSA) aren't code; the audio
// callback's brief lock-in-condition is deliberate; and the unit tests
// compare exact f32 values. The decode/output paths' real robustness
// (poisoned-lock recovery, graceful thread-spawn failure) is handled in
// code below, not suppressed. Mirrors the inline-allow idiom used for
// DSP math elsewhere (e.g. start_menu.rs).
#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::suboptimal_flops,
    clippy::significant_drop_in_scrutinee,
    clippy::float_cmp,
    clippy::too_long_first_doc_paragraph,
    clippy::default_trait_access,
    clippy::missing_const_for_fn
)]

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use symphonia::core::audio::{SampleBuffer, SignalSpec};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Gapless pre-buffer lead (ms): the higher-level queue driver (AIR-2.c)
/// starts resolving the next track's stream URL once the current track
/// has this much or less remaining (R— AIR-5 lock). [`Engine::near_end`]
/// exposes the signal; the engine's own `play(list)` is already gapless
/// without it.
pub const GAPLESS_LEAD_MS: u64 = 5_000;

// ───────────────────────── pure helpers ─────────────────────────

/// Source container/codec inferred from a track's file suffix. Drives
/// the Symphonia probe [`Hint`] (a hint only speeds + disambiguates
/// probing — the actual format is verified from the bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceCodec {
    /// FLAC (`.flac`).
    Flac,
    /// MPEG-1/2 Layer III (`.mp3`).
    Mp3,
    /// Ogg Vorbis (`.ogg`).
    Vorbis,
    /// AAC, typically in an MP4/M4A container (`.m4a` / `.aac`).
    Aac,
    /// PCM WAV (`.wav`).
    Wav,
    /// Opus — decoded in AIR-5.b via libopus; unsupported by this build.
    Opus,
    /// Unknown suffix: probe from the bytes with no extension hint.
    Unknown,
}

impl SourceCodec {
    /// Classify from a Subsonic `suffix` (or a filename extension).
    #[must_use]
    pub fn from_suffix(suffix: &str) -> Self {
        match suffix
            .trim()
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str()
        {
            "flac" => Self::Flac,
            "mp3" => Self::Mp3,
            "ogg" | "oga" | "vorbis" => Self::Vorbis,
            "aac" | "m4a" | "mp4" | "alac" => Self::Aac,
            "wav" | "wave" => Self::Wav,
            "opus" => Self::Opus,
            _ => Self::Unknown,
        }
    }

    /// The Symphonia probe extension hint (`None` when there's nothing
    /// useful to hint with).
    #[must_use]
    pub fn hint_ext(self) -> Option<&'static str> {
        match self {
            Self::Flac => Some("flac"),
            Self::Mp3 => Some("mp3"),
            Self::Vorbis => Some("ogg"),
            Self::Aac => Some("m4a"),
            Self::Wav => Some("wav"),
            Self::Opus | Self::Unknown => None,
        }
    }

    /// Whether this build can decode the codec (everything but Opus,
    /// which is AIR-5.b).
    #[must_use]
    pub fn is_supported(self) -> bool {
        !matches!(self, Self::Opus)
    }
}

/// Should the queue driver begin pre-buffering the next track? True once
/// the current track is within [`GAPLESS_LEAD_MS`] of its end (and its
/// duration is known).
#[must_use]
pub fn should_prebuffer_next(position_ms: u64, duration_ms: u64, lead_ms: u64) -> bool {
    duration_ms > 0 && duration_ms.saturating_sub(position_ms) <= lead_ms
}

/// Clamp a volume multiplier into the valid `0.0..=1.0` range.
#[must_use]
pub fn clamp_volume(v: f32) -> f32 {
    v.clamp(0.0, 1.0)
}

/// One output sample for the cpal callback: the next ring sample scaled
/// by `volume` when playing, or `None` (→ the callback writes silence and
/// does not advance the playhead) when paused or on a buffer underrun.
#[must_use]
pub fn pull_sample(ring: &mut VecDeque<f32>, playing: bool, volume: f32) -> Option<f32> {
    if !playing {
        return None;
    }
    ring.pop_front().map(|s| s * clamp_volume(volume))
}

/// Linear-interpolation resample of interleaved `input` from `src_rate`
/// to `dst_rate`. A first-pass resampler — good enough to verify the
/// pipeline; the HW bench judges audio quality and drives any upgrade to
/// a windowed-sinc resampler. Returns `input` unchanged when the rates
/// match or an argument is degenerate.
#[must_use]
pub fn resample_linear(input: &[f32], channels: usize, src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if channels == 0 || input.is_empty() || src_rate == 0 || dst_rate == 0 || src_rate == dst_rate {
        return input.to_vec();
    }
    let frames_in = input.len() / channels;
    if frames_in == 0 {
        return input.to_vec();
    }
    let frames_out = (frames_in as u64 * u64::from(dst_rate) / u64::from(src_rate)) as usize;
    let mut out = Vec::with_capacity(frames_out * channels);
    let ratio = f64::from(src_rate) / f64::from(dst_rate);
    for f in 0..frames_out {
        let src_pos = f as f64 * ratio;
        let i0 = src_pos.floor() as usize;
        let frac = (src_pos - i0 as f64) as f32;
        let i1 = (i0 + 1).min(frames_in - 1);
        for c in 0..channels {
            let a = input[i0 * channels + c];
            let b = input[i1 * channels + c];
            out.push(a + (b - a) * frac);
        }
    }
    out
}

/// Map interleaved `input` from `src_ch` channels to `dst_ch`: mono is
/// up-mixed by duplication, anything-to-mono is down-mixed by averaging,
/// and other mismatches copy the overlapping channels (padding with
/// silence). Returns `input` unchanged when the counts match.
#[must_use]
pub fn map_channels(input: &[f32], src_ch: usize, dst_ch: usize) -> Vec<f32> {
    if src_ch == 0 || dst_ch == 0 || src_ch == dst_ch {
        return input.to_vec();
    }
    let frames = input.len() / src_ch;
    let mut out = Vec::with_capacity(frames * dst_ch);
    for f in 0..frames {
        let frame = &input[f * src_ch..f * src_ch + src_ch];
        if src_ch == 1 {
            for _ in 0..dst_ch {
                out.push(frame[0]);
            }
        } else if dst_ch == 1 {
            out.push(frame.iter().sum::<f32>() / src_ch as f32);
        } else {
            for c in 0..dst_ch {
                out.push(frame.get(c).copied().unwrap_or(0.0));
            }
        }
    }
    out
}

// ───────────────────────── engine ─────────────────────────

/// State shared between the audio callback, the decode thread, and the
/// owning [`Engine`]. All fields are lock-free atomics except the sample
/// ring, which is a short critical section on each callback / decode push.
struct Shared {
    /// Decoded, device-rate, device-channel interleaved f32 samples.
    ring: Mutex<VecDeque<f32>>,
    /// Volume multiplier, stored as `f32::to_bits` (atomic).
    volume: AtomicU32,
    /// Play / pause. When false the callback emits silence without
    /// draining the ring, so resume is seamless.
    playing: AtomicBool,
    /// Stop signal for the decode thread.
    stop: AtomicBool,
    /// Set true when the decode thread has finished the whole track list.
    decode_done: AtomicBool,
    /// Device frames actually emitted (drives the playhead).
    frames_played: AtomicU64,
    device_rate: u32,
    device_channels: u16,
    /// Back-pressure target: the decode thread throttles once the ring
    /// holds more than this many samples (≈2 s of audio).
    target_ring: usize,
}

/// The native playback engine: a live cpal output stream fed by a decode
/// thread. Construct once (it grabs the default output device), then
/// drive it with [`play`](Engine::play) / [`pause`](Engine::pause) /
/// [`stop`](Engine::stop).
pub struct Engine {
    shared: Arc<Shared>,
    decode: Mutex<Option<JoinHandle<()>>>,
    /// Kept alive for the engine's lifetime — dropping it stops audio.
    _stream: cpal::Stream,
}

impl Engine {
    /// Open the default output device and start its (initially silent)
    /// stream.
    ///
    /// # Errors
    /// No output device, an unsupported device sample format, or a
    /// stream-build/-start failure.
    pub fn new() -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let supported = device
            .default_output_config()
            .map_err(|e| format!("query output config: {e}"))?;
        let sample_format = supported.sample_format();
        let config: cpal::StreamConfig = supported.config();
        let device_rate = config.sample_rate; // cpal 0.17: SampleRate = u32
        let device_channels = config.channels;
        let target_ring = (device_rate as usize) * (device_channels as usize) * 2;

        let shared = Arc::new(Shared {
            ring: Mutex::new(VecDeque::new()),
            volume: AtomicU32::new(1.0_f32.to_bits()),
            playing: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            decode_done: AtomicBool::new(true),
            frames_played: AtomicU64::new(0),
            device_rate,
            device_channels,
            target_ring,
        });

        let stream = match sample_format {
            cpal::SampleFormat::F32 => build_output_stream::<f32>(&device, &config, shared.clone()),
            cpal::SampleFormat::I16 => build_output_stream::<i16>(&device, &config, shared.clone()),
            cpal::SampleFormat::U16 => build_output_stream::<u16>(&device, &config, shared.clone()),
            other => return Err(format!("unsupported device sample format: {other:?}")),
        }
        .map_err(|e| format!("build output stream: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("start output stream: {e}"))?;

        Ok(Self {
            shared,
            decode: Mutex::new(None),
            _stream: stream,
        })
    }

    /// Play the given tracks back-to-back, gaplessly. Each entry is a
    /// stream URL plus its (hinted) codec. Replaces any current playback.
    pub fn play(&self, tracks: Vec<(String, SourceCodec)>) {
        self.stop();
        if tracks.is_empty() {
            return;
        }
        self.shared.stop.store(false, Ordering::Relaxed);
        self.shared.playing.store(true, Ordering::Relaxed);
        self.shared.frames_played.store(0, Ordering::Relaxed);
        self.shared.decode_done.store(false, Ordering::Relaxed);

        let shared = self.shared.clone();
        let handle = std::thread::Builder::new()
            .name("mde-musicd-decode".to_string())
            .spawn(move || {
                for (url, codec) in tracks {
                    if shared.stop.load(Ordering::Relaxed) {
                        break;
                    }
                    if let Err(e) = decode_track(&url, codec, &shared) {
                        eprintln!("mde-musicd: {e}");
                    }
                }
                shared.decode_done.store(true, Ordering::Relaxed);
            });
        match handle {
            Ok(joined) => {
                *self.decode.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(joined);
            }
            Err(e) => {
                eprintln!("mde-musicd: could not start decode thread: {e}");
                // Nothing will play — let the playhead/idle checks settle.
                self.shared.decode_done.store(true, Ordering::Relaxed);
                self.shared.playing.store(false, Ordering::Relaxed);
            }
        }
    }

    /// Pause output (the ring is preserved; [`resume`](Engine::resume)
    /// continues seamlessly).
    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    /// Resume after a [`pause`](Engine::pause).
    pub fn resume(&self) {
        self.shared.playing.store(true, Ordering::Relaxed);
    }

    /// Stop playback: signal + join the decode thread and clear the ring.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.playing.store(false, Ordering::Relaxed);
        if let Some(handle) = self.decode.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            let _ = handle.join();
        }
        self.shared.ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
        self.shared.decode_done.store(true, Ordering::Relaxed);
    }

    /// Set the volume multiplier (clamped to `0.0..=1.0`).
    pub fn set_volume(&self, v: f32) {
        self.shared
            .volume
            .store(clamp_volume(v).to_bits(), Ordering::Relaxed);
    }

    /// The current volume multiplier.
    #[must_use]
    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    /// Playhead position (ms), derived from device frames emitted.
    #[must_use]
    pub fn position_ms(&self) -> u64 {
        let frames = self.shared.frames_played.load(Ordering::Relaxed);
        if self.shared.device_rate == 0 {
            0
        } else {
            frames * 1000 / u64::from(self.shared.device_rate)
        }
    }

    /// Whether anything is still playing or buffered.
    #[must_use]
    pub fn is_active(&self) -> bool {
        !self.shared.decode_done.load(Ordering::Relaxed)
            || !self.shared.ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner).is_empty()
    }

    /// Is the current track within [`GAPLESS_LEAD_MS`] of its end? The
    /// signal the queue driver (AIR-2.c) uses to resolve the next track.
    #[must_use]
    pub fn near_end(&self, track_duration_ms: u64) -> bool {
        should_prebuffer_next(self.position_ms(), track_duration_ms, GAPLESS_LEAD_MS)
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.decode.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take() {
            let _ = handle.join();
        }
    }
}

/// Build a typed cpal output stream whose callback drains the shared ring
/// (per the [`pull_sample`] contract) and counts emitted frames toward the
/// playhead. `T` is the device's native sample type.
fn build_output_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Shared>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let channels = shared.device_channels.max(1) as usize;
    device.build_output_stream(
        config,
        move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
            let playing = shared.playing.load(Ordering::Relaxed);
            let volume = f32::from_bits(shared.volume.load(Ordering::Relaxed));
            let mut real = 0usize;
            {
                let mut ring = shared.ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
                for slot in out.iter_mut() {
                    match pull_sample(&mut ring, playing, volume) {
                        Some(s) => {
                            *slot = T::from_sample(s);
                            real += 1;
                        }
                        None => *slot = T::from_sample(0.0),
                    }
                }
            }
            shared
                .frames_played
                .fetch_add((real / channels) as u64, Ordering::Relaxed);
        },
        |err| eprintln!("mde-musicd: audio stream error: {err}"),
        None,
    )
}

/// Fetch, decode, resample, channel-map, and enqueue one track's samples
/// into the shared ring. Returns when the track is exhausted or `stop` is
/// signalled.
fn decode_track(url: &str, codec: SourceCodec, shared: &Shared) -> Result<(), String> {
    if !codec.is_supported() {
        return Err(format!(
            "{url}: opus is not supported by this build (Symphonia 0.5 ships no opus decoder; tracked as AIR-5.b)"
        ));
    }

    let bytes = reqwest::blocking::get(url)
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::bytes)
        .map_err(|e| format!("fetch {url}: {e}"))?
        .to_vec();

    let mss = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = codec.hint_ext() {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("probe {url}: {e}"))?;
    let mut format = probed.format;

    let track = format
        .default_track()
        .filter(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .or_else(|| {
            format
                .tracks()
                .iter()
                .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        })
        .ok_or_else(|| format!("{url}: no decodable audio track"))?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("decoder for {url}: {e}"))?;

    let dst_rate = shared.device_rate;
    let dst_ch = shared.device_channels as usize;

    loop {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        // End of stream (UnexpectedEof) or a fatal reset — this track is
        // done; the caller advances to the next one gaplessly.
        let Ok(packet) = format.next_packet() else {
            break;
        };
        if packet.track_id() != track_id {
            continue;
        }
        let audio_ref = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymphoniaError::DecodeError(_)) => continue, // recoverable
            Err(_) => break,
        };
        let spec: SignalSpec = *audio_ref.spec();
        let cap = audio_ref.capacity() as u64;
        if cap == 0 {
            continue;
        }
        let mut sample_buf = SampleBuffer::<f32>::new(cap, spec);
        sample_buf.copy_interleaved_ref(audio_ref);
        let src_ch = spec.channels.count().max(1);
        let resampled = resample_linear(sample_buf.samples(), src_ch, spec.rate, dst_rate);
        let mapped = map_channels(&resampled, src_ch, dst_ch);

        // Back-pressure: keep the ring bounded so we don't decode an
        // entire FLAC into RAM ahead of the playhead.
        while !shared.stop.load(Ordering::Relaxed)
            && shared.ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner).len() > shared.target_ring
        {
            std::thread::sleep(Duration::from_millis(8));
        }
        shared.ring.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(mapped);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_from_suffix_classifies() {
        assert_eq!(SourceCodec::from_suffix("flac"), SourceCodec::Flac);
        assert_eq!(SourceCodec::from_suffix("song.MP3"), SourceCodec::Mp3);
        assert_eq!(SourceCodec::from_suffix("ogg"), SourceCodec::Vorbis);
        assert_eq!(SourceCodec::from_suffix("track.m4a"), SourceCodec::Aac);
        assert_eq!(SourceCodec::from_suffix("wav"), SourceCodec::Wav);
        assert_eq!(SourceCodec::from_suffix("opus"), SourceCodec::Opus);
        assert_eq!(SourceCodec::from_suffix("xyz"), SourceCodec::Unknown);
    }

    #[test]
    fn codec_hint_and_support() {
        assert_eq!(SourceCodec::Flac.hint_ext(), Some("flac"));
        assert_eq!(SourceCodec::Vorbis.hint_ext(), Some("ogg"));
        assert_eq!(SourceCodec::Unknown.hint_ext(), None);
        assert_eq!(SourceCodec::Opus.hint_ext(), None);
        // Opus is the one codec this build can't decode (AIR-5.b).
        assert!(SourceCodec::Flac.is_supported());
        assert!(!SourceCodec::Opus.is_supported());
    }

    #[test]
    fn prebuffer_fires_only_within_lead() {
        // 4:00 track, 3:54 in → 6 s left → not yet (lead 5 s).
        assert!(!should_prebuffer_next(234_000, 240_000, GAPLESS_LEAD_MS));
        // 3:55.1 in → 4.9 s left → fire.
        assert!(should_prebuffer_next(235_100, 240_000, GAPLESS_LEAD_MS));
        // Exactly at the lead boundary → fire.
        assert!(should_prebuffer_next(235_000, 240_000, GAPLESS_LEAD_MS));
        // Unknown duration → never.
        assert!(!should_prebuffer_next(1_000, 0, GAPLESS_LEAD_MS));
        // Past the end → fire.
        assert!(should_prebuffer_next(999_999, 240_000, GAPLESS_LEAD_MS));
    }

    #[test]
    fn volume_clamps() {
        assert_eq!(clamp_volume(-0.5), 0.0);
        assert_eq!(clamp_volume(0.3), 0.3);
        assert_eq!(clamp_volume(2.0), 1.0);
    }

    #[test]
    fn pull_sample_plays_pauses_and_underruns() {
        let mut ring = VecDeque::from([1.0_f32, 0.5]);
        // Playing at half volume → scaled sample, ring advances.
        assert_eq!(pull_sample(&mut ring, true, 0.5), Some(0.5));
        assert_eq!(ring.len(), 1);
        // Paused → silence, ring preserved.
        assert_eq!(pull_sample(&mut ring, false, 1.0), None);
        assert_eq!(ring.len(), 1);
        // Drain the last, then underrun → None.
        assert_eq!(pull_sample(&mut ring, true, 1.0), Some(0.5));
        assert_eq!(pull_sample(&mut ring, true, 1.0), None);
    }

    #[test]
    fn resample_identity_up_and_down() {
        let stereo = [0.0, 1.0, 0.2, 0.8, 0.4, 0.6, 0.6, 0.4]; // 4 frames, 2ch
        // Same rate → identity.
        assert_eq!(resample_linear(&stereo, 2, 48_000, 48_000), stereo.to_vec());
        // Upsample 2× → ~double the frames.
        let up = resample_linear(&stereo, 2, 24_000, 48_000);
        assert_eq!(up.len() / 2, 8);
        // First output frame equals the first input frame.
        assert!((up[0] - 0.0).abs() < 1e-6 && (up[1] - 1.0).abs() < 1e-6);
        // Downsample 2× → ~half the frames.
        let down = resample_linear(&stereo, 2, 48_000, 24_000);
        assert_eq!(down.len() / 2, 2);
        // Empty + degenerate inputs pass through.
        assert!(resample_linear(&[], 2, 48_000, 24_000).is_empty());
        assert_eq!(resample_linear(&stereo, 2, 0, 24_000), stereo.to_vec());
    }

    #[test]
    fn channel_map_up_down_and_identity() {
        // Mono → stereo duplicates each sample.
        assert_eq!(map_channels(&[0.1, 0.2], 1, 2), vec![0.1, 0.1, 0.2, 0.2]);
        // Stereo → mono averages the pair.
        assert_eq!(map_channels(&[0.0, 1.0, 0.4, 0.6], 2, 1), vec![0.5, 0.5]);
        // Equal counts → identity.
        assert_eq!(map_channels(&[0.3, 0.7], 2, 2), vec![0.3, 0.7]);
        // Degenerate → passthrough.
        assert_eq!(map_channels(&[0.3, 0.7], 0, 2), vec![0.3, 0.7]);
    }
}
