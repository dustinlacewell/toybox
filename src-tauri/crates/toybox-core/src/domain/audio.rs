use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioMeta {
    pub file: String,
    pub format: String,
    pub duration: f64,
    pub sample_rate: u32,
    pub channels: usize,
    pub bit_depth: Option<u32>,
    pub byte_length: u64,
    pub modified_ns: String,
    /// Maximum absolute amplitude per bin, across all channels.
    pub peaks: Vec<f32>,
}
