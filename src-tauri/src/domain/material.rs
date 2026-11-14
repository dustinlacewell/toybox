//! Material sets: pure data, no IO. A material is a set of texture maps, each
//! bound to a role (albedo, normal, ...) and optionally to one channel of a
//! file, so packed textures (e.g. ORM) can feed several roles from one image.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapRole {
    Albedo,
    Normal,
    Roughness,
    Metallic,
    Ao,
    Height,
    Emissive,
    Opacity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MapChannel {
    R,
    G,
    B,
    A,
}

/// One role of a material, bound to a texture file and optionally to one channel of it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialMap {
    pub role: MapRole,
    /// Texture path relative to `library_root`. Several roles may share one file (packed textures).
    pub file: String,
    /// The channel a scalar role reads. `None` = the whole image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<MapChannel>,
    /// Scalar role stored inverted (e.g. smoothness in place of roughness).
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterialSet {
    pub maps: Vec<MaterialMap>,
    /// Normal map uses the DirectX convention (green flipped).
    #[serde(default)]
    pub flip_normal_green: bool,
}

impl MaterialSet {
    /// Unique texture files, in first-seen order.
    pub fn textures(&self) -> Vec<String> {
        let mut files: Vec<String> = Vec::new();
        for map in &self.maps {
            if !files.contains(&map.file) {
                files.push(map.file.clone());
            }
        }
        files
    }

    /// A material needs an albedo map, and each role may appear only once.
    pub fn validate(&self) -> AppResult<()> {
        if !self.maps.iter().any(|m| m.role == MapRole::Albedo) {
            return Err(AppError::msg("material has no albedo map"));
        }
        for (i, map) in self.maps.iter().enumerate() {
            if self.maps[..i].iter().any(|m| m.role == map.role) {
                return Err(AppError::msg(format!("material has role {:?} twice", map.role)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(role: MapRole, file: &str) -> MaterialMap {
        MaterialMap { role, file: file.into(), channel: None, invert: false }
    }

    fn set(maps: Vec<MaterialMap>) -> MaterialSet {
        MaterialSet { maps, flip_normal_green: false }
    }

    #[test]
    fn textures_are_unique_in_first_seen_order() {
        let s = set(vec![
            map(MapRole::Albedo, "a.png"),
            map(MapRole::Roughness, "orm.png"),
            map(MapRole::Ao, "orm.png"),
            map(MapRole::Normal, "n.png"),
        ]);
        assert_eq!(s.textures(), vec!["a.png", "orm.png", "n.png"]);
    }

    #[test]
    fn validate_accepts_albedo_with_unique_roles() {
        let s = set(vec![map(MapRole::Albedo, "a.png"), map(MapRole::Normal, "n.png")]);
        assert!(s.validate().is_ok());
    }

    #[test]
    fn validate_rejects_missing_albedo() {
        assert!(set(vec![map(MapRole::Normal, "n.png")]).validate().is_err());
    }

    #[test]
    fn validate_rejects_duplicate_role() {
        let s = set(vec![map(MapRole::Albedo, "a.png"), map(MapRole::Albedo, "b.png")]);
        assert!(s.validate().is_err());
    }
}
