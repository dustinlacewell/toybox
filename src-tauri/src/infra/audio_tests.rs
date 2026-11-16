use super::audio::*;
use crate::{
    domain::{
        catalog_model::{AssetContent, Catalog},
        seed,
    },
    infra::fsio,
};
use serde_json::json;

fn wav(channels: u16, rate: u32, frames: u32) -> Vec<u8> {
    let size = frames * channels as u32 * 2;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + size).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(channels.to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * channels as u32 * 2).to_le_bytes());
    out.extend((channels * 2).to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(size.to_le_bytes());
    for frame in 0..frames {
        for _ in 0..channels {
            let sample = if frame % 2 == 0 { 16384i16 } else { -16384 };
            out.extend(sample.to_le_bytes());
        }
    }
    out
}

#[test]
fn audio_indexing_reuses_valid_facts_detects_changes_and_survives_rescan_and_delete() {
    let folder = std::env::temp_dir().join(format!("toybox-audio-index-{}", std::process::id()));
    let root = folder.join("root");
    let relative = "library/test/sounds/tone.wav";
    let path = root.join(relative);
    let bytes = wav(2, 8000, 1600);
    fsio::write_bytes(&path, &bytes).unwrap();
    let meta = probe(&root, relative).unwrap();
    assert_eq!(meta.channels, 2);
    assert_eq!(meta.sample_rate, 8000);
    assert_eq!(meta.bit_depth, Some(16));
    assert!((meta.duration - 0.2).abs() < 1e-6);
    assert!(meta.peaks.len() > 1);
    assert!(meta.peaks.iter().all(|v| (*v - 0.5).abs() < 0.001));
    assert_eq!(
        probe(&root, relative).unwrap().modified_ns,
        meta.modified_ns
    );
    let manifest = json!({"schemaVersion":1,"assets":[{"kind":"audio","id":"tone","pack":"test","category":"sounds","file":"tone.wav"}]});
    fsio::write_bytes(
        &root.join(crate::config::SEED_REL),
        &serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let build = || {
        crate::commands::catalog_build::build_catalog(
            root.to_str().unwrap(),
            seed::parse_seed_entries(&manifest).unwrap(),
        )
        .unwrap()
    };
    let catalog = build();
    assert_eq!(catalog.assets[0].name, "tone");
    assert_eq!(build().assets.len(), 1);
    let cache = folder.join("catalog.json");
    fsio::write_bytes(&cache, &serde_json::to_vec(&catalog).unwrap()).unwrap();
    let mut report = crate::domain::export_model::ExportReport::default();
    let target = folder.join("export");
    crate::commands::export_copy::copy_audio(
        root.to_str().unwrap(),
        &target,
        &catalog.assets[0],
        "tone",
        true,
        &mut report,
    )
    .unwrap();
    assert_eq!(report.written, vec!["test/sounds/tone.wav"]);
    assert_eq!(
        fsio::read_bytes(&target.join(&report.written[0])).unwrap(),
        bytes
    );
    crate::commands::export_copy::copy_audio(
        root.to_str().unwrap(),
        &target,
        &catalog.assets[0],
        "renamed",
        false,
        &mut report,
    )
    .unwrap();
    assert_eq!(
        fsio::read_bytes(&target.join("renamed.wav")).unwrap(),
        bytes
    );
    std::fs::write(&path, b"not audio").unwrap();
    assert!(probe(&root, relative).is_err());
    std::fs::write(&path, &bytes).unwrap();
    assert!(probe(&root, relative).is_ok());
    let outside = folder.join("outside.wav");
    std::fs::write(&outside, &bytes).unwrap();
    assert!(probe(&root, "../outside.wav").is_err());
    let result =
        crate::commands::delete::delete_from_library(&root, &cache, &["tone".into()]).unwrap();
    assert!(result.assets.is_empty());
    assert!(!path.exists());
    assert!(outside.exists());
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
#[ignore = "requires TOYBOX_AUDIO_FIXTURES, writes browser fixture metadata"]
fn compressed_audio_and_waveform_fixtures() {
    let folder = std::path::PathBuf::from(std::env::var_os("TOYBOX_AUDIO_FIXTURES").unwrap());
    let root = folder.join("root");
    let entries:Vec<_>=["wav","mp3","ogg","flac"].into_iter().map(|ext|json!({"kind":"audio","id":ext,"pack":"test","category":"sounds","file":format!("tone.{ext}")})).collect();
    let catalog = crate::commands::catalog_build::build_catalog(
        root.to_str().unwrap(),
        seed::parse_seed_entries(&json!({"assets":entries})).unwrap(),
    )
    .unwrap();
    for asset in &catalog.assets {
        let AssetContent::Audio { audio } = &asset.content else {
            panic!()
        };
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.sample_rate, 44100);
        assert!((audio.duration - 2.0).abs() < 0.1);
        assert_eq!(audio.peaks.len(), 512);
        assert!(audio.peaks.iter().all(|v| v.is_finite()));
        println!(
            "{}: {:.3}s, {} peaks",
            audio.format,
            audio.duration,
            audio.peaks.len()
        );
    }
    fsio::write_bytes(
        &folder.join("catalog.json"),
        &serde_json::to_vec(&catalog).unwrap(),
    )
    .unwrap();
    let round: Catalog =
        serde_json::from_slice(&fsio::read_bytes(&folder.join("catalog.json")).unwrap()).unwrap();
    assert_eq!(round.assets.len(), 4);
}
