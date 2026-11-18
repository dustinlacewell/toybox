//! Streaming decode keeps waveform indexing bounded even for long music files.
use crate::{
    domain::{
        audio::AudioMeta,
        catalog_model::{Asset, AssetContent},
    },
    error::{AppError, AppResult},
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
use symphonia::core::{
    audio::SampleBuffer, codecs::DecoderOptions, errors::Error, formats::FormatOptions,
    io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
};
static CACHE: OnceLock<Mutex<HashMap<PathBuf, AudioMeta>>> = OnceLock::new();
fn cache() -> &'static Mutex<HashMap<PathBuf, AudioMeta>> {
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Seed the metadata cache from persisted facts. File signatures are checked
/// again on every probe, so rescans reuse waveforms only for unchanged files.
pub fn remember(root: &Path, assets: &[Asset]) {
    if let Ok(mut cache) = cache().lock() {
        for asset in assets {
            for item in std::iter::once(asset).chain(asset.variants.iter()) {
                if let AssetContent::Audio { audio } = &item.content {
                    if let Ok(path) = root.join(&audio.file).canonicalize() {
                        cache.entry(path).or_insert_with(|| audio.clone());
                    }
                }
            }
        }
    }
}

pub fn probe(root: &Path, relative: &str) -> AppResult<AudioMeta> {
    let root = root
        .canonicalize()
        .map_err(|e| AppError::file("Resolve library", root, e))?;
    let path = root
        .join(relative)
        .canonicalize()
        .map_err(|e| AppError::file("Read audio", &root.join(relative), e))?;
    if Path::new(relative).is_absolute() || !path.starts_with(&root.join("library")) {
        return Err(AppError::msg("Audio must be inside library asset files"));
    }
    let metadata =
        std::fs::metadata(&path).map_err(|e| AppError::file("Read audio metadata", &path, e))?;
    let stamp = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_string();
    if let Some(prior) = cache()
        .lock()
        .unwrap()
        .get(&path)
        .filter(|a| a.byte_length == metadata.len() && a.modified_ns == stamp)
        .cloned()
    {
        return Ok(AudioMeta {
            file: relative.into(),
            ..prior
        });
    }
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase();
    if !["wav", "mp3", "ogg", "flac"].contains(&ext.as_str()) {
        return Err(AppError::msg(
            "Supported audio formats: WAV, MP3, Ogg Vorbis and FLAC",
        ));
    }
    let file = std::fs::File::open(&path).map_err(|e| AppError::file("Open audio", &path, e))?;
    let mut hint = Hint::new();
    hint.with_extension(&ext);
    let mut format = symphonia::default::get_probe()
        .format(
            &hint,
            MediaSourceStream::new(Box::new(file), Default::default()),
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| AppError::msg(format!("Cannot read audio '{}': {e}", path.display())))?
        .format;
    let track = format
        .default_track()
        .ok_or_else(|| AppError::msg("Audio file has no playable track"))?;
    let track_id = track.id;
    let bit_depth = track.codec_params.bits_per_sample;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| {
            AppError::msg(format!(
                "Unsupported audio codec in '{}': {e}",
                path.display()
            ))
        })?;
    let mut peaks = Vec::<f32>::new();
    let mut block = 64usize;
    let mut count = 0usize;
    let mut peak = 0f32;
    let mut frames = 0u64;
    let mut sample_rate = 0;
    let mut channels = 0;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => {
                return Err(AppError::msg(format!(
                    "Cannot read audio packets '{}': {e}",
                    path.display()
                )))
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| AppError::msg(format!("Cannot decode audio '{}': {e}", path.display())))?;
        let spec = *decoded.spec();
        let n = spec.channels.count();
        if n == 0 || spec.rate == 0 {
            return Err(AppError::msg("Audio has invalid channels or sample rate"));
        }
        if sample_rate != 0 && (sample_rate != spec.rate || channels != n) {
            return Err(AppError::msg(
                "Audio changes sample rate or channels within the track",
            ));
        }
        sample_rate = spec.rate;
        channels = n;
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        samples.copy_interleaved_ref(decoded);
        for frame in samples.samples().chunks_exact(n) {
            frames += 1;
            count += 1;
            for &sample in frame {
                if sample.is_finite() {
                    peak = peak.max(sample.abs().min(1.0));
                }
            }
            if count == block {
                peaks.push(peak);
                count = 0;
                peak = 0.;
                if peaks.len() == 4096 {
                    peaks = peaks.chunks_exact(2).map(|v| v[0].max(v[1])).collect();
                    block *= 2;
                }
            }
        }
    }
    if frames == 0 || sample_rate == 0 || channels == 0 {
        return Err(AppError::msg("Audio file contains no decodable samples"));
    }
    if count > 0 {
        peaks.push(peak);
    }
    let bins = 512.min(peaks.len());
    let waveform = (0..bins)
        .map(|i| {
            peaks[i * peaks.len() / bins
                ..((i + 1) * peaks.len() / bins).max(i * peaks.len() / bins + 1)]
                .iter()
                .copied()
                .fold(0f32, f32::max)
        })
        .collect();
    let audio = AudioMeta {
        file: relative.into(),
        format: ext.to_uppercase(),
        duration: frames as f64 / sample_rate as f64,
        sample_rate,
        channels,
        bit_depth,
        byte_length: metadata.len(),
        modified_ns: stamp,
        peaks: waveform,
    };
    cache().lock().unwrap().insert(path, audio.clone());
    Ok(audio)
}
