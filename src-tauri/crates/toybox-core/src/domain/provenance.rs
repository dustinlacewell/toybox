//! Source information is metadata, never a library path or storage instruction.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImportProvenance {
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub filesystem_path: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub publisher: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if="Option::is_none")]
    pub license: Option<String>,
}

impl ImportProvenance {
    pub fn normalized(mut self) -> Option<Self> {
        for field in [&mut self.filesystem_path,&mut self.source,&mut self.publisher,&mut self.url,&mut self.license] {
            *field=field.take().and_then(|value| { let value=value.trim(); if value.is_empty(){None}else{Some(value.to_string())} });
        }
        if self==Self::default(){None}else{Some(self)}
    }

    pub fn tags(&self) -> Vec<String> {
        [("source", &self.source),("publisher", &self.publisher),("license", &self.license)]
            .into_iter().filter_map(|(key,value)|value.as_ref().map(|value|format!("{key}: {value}"))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use crate::domain::{seed,merge,catalog_model::{Catalog,AnimationMeta}};

    #[test]
    fn provenance_survives_manifest_rescan_and_updates_tags_without_losing_user_metadata() {
        let provenance=json!({"filesystemPath":" D:\\downloads\\Pack.zip ","source":" Space Pack ","publisher":" Studio ","url":"https://example.test/pack","license":" CC0 "});
        let entry=json!({"id":"asset","pack":"storage","category":"props","file":"Model.gltf","provenance":provenance,
            "variants":[{"id":"low","pack":"storage","category":"props","file":"Model_LOD1.gltf"}]});
        let parsed=seed::parse_entry(&entry).unwrap();
        assert_eq!(parsed.provenance,parsed.variants[0].provenance);
        let mut asset=seed::build_asset(parsed,vec![],AnimationMeta::default()).unwrap();
        assert_eq!(asset.user.tags,vec!["source: Space Pack","publisher: Studio","license: CC0"]);
        assert_eq!(asset.user.provenance.as_ref().unwrap().filesystem_path.as_deref(),Some("D:\\downloads\\Pack.zip"));
        asset.user.favorite=true;asset.user.tags.push("custom".into());asset.user.collections.push("Project".into());
        let prior=Catalog {rigs:vec![],schema_version:5,library_root:"root".into(),assets:vec![asset]};
        let manifest=seed::upsert_entries(json!({"assets":[entry]}),vec![json!({"id":"asset","pack":"storage","category":"props","file":"Model.gltf"})]).unwrap();
        assert_eq!(manifest["assets"][0]["provenance"],provenance,"reimport without fields retains manifest provenance");
        let rebuilt=seed::build_asset(seed::parse_entry(&manifest["assets"][0]).unwrap(),vec![],AnimationMeta::default()).unwrap();
        let rescanned=merge::upsert_assets(prior,vec![rebuilt]);
        assert!(rescanned.assets[0].user.favorite);assert!(rescanned.assets[0].user.tags.contains(&"custom".into()));
        let mut updated=manifest["assets"][0].clone();updated["provenance"]["license"]=json!("CC BY 4.0");
        let rebuilt=seed::build_asset(seed::parse_entry(&updated).unwrap(),vec![],AnimationMeta::default()).unwrap();
        let merged=merge::upsert_assets(rescanned,vec![rebuilt]);
        let user=&merged.assets[0].user;
        assert!(user.tags.contains(&"license: CC BY 4.0".into()));assert!(!user.tags.contains(&"license: CC0".into()));
        assert!(user.tags.contains(&"custom".into()));assert_eq!(user.collections,vec!["Project"]);
        let roundtrip:Catalog=serde_json::from_value(serde_json::to_value(&merged).unwrap()).unwrap();
        assert_eq!(roundtrip.assets[0].user.provenance,user.provenance);
    }

    #[test]
    fn all_types_accept_provenance_and_empty_fields_create_no_tags() {
        for entry in [
            json!({"id":"m","pack":"p","category":"c","file":"M.gltf"}),
            json!({"kind":"animation","id":"a","pack":"p","category":"c","file":"A.gltf"}),
            json!({"kind":"audio","id":"s","pack":"p","category":"c","file":"S.wav"}),
            json!({"kind":"material","id":"t","pack":"p","category":"c","name":"T","maps":[{"role":"albedo","file":"color.png"}]})
        ] {
            let mut entry=entry;entry["provenance"]=json!({"source":" Pack ","publisher":"  "});
            let parsed=seed::parse_entry(&entry).unwrap();
            assert_eq!(parsed.provenance.unwrap().tags(),vec!["source: Pack"]);
        }
        assert!(ImportProvenance::default().normalized().is_none());
    }
}
