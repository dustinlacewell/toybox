//! Pure domain layer: data types and transforms with no IO or framework deps.

pub mod catalog_migrate;
pub mod library_migrations;
pub mod animation;
pub mod audio;
pub mod animation_extract;
pub mod animation_preview;
pub mod rig;
pub mod catalog_model;
pub mod export_copy_plan;
pub mod export_model;
pub mod glb_assemble;
pub mod gltf_origin;
pub mod gltf_parse;
pub mod gltf_uri;
pub mod gltf_vertex_colors;
pub mod image_embed;
pub mod mat4;
pub mod material;
pub mod merge;
pub mod pack_meta;
pub mod paths;
pub mod placer_library;
pub mod seed;
pub mod provenance;
pub mod tags;
