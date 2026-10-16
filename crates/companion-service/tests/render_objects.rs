//! Independent Rust checks of the exact TypeScript-generated ABI/hash vectors.
//! These do not admit a render package or authorize semantic input.
use osg_companion_service::render_object::{inspect_render_object, RenderObjectKind};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/companion-renderer/test/fixtures")
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn kind(name: &str) -> RenderObjectKind {
    match name {
        "semantics" => RenderObjectKind::Semantics,
        "scene" => RenderObjectKind::Scene,
        "images" => RenderObjectKind::Images,
        "fonts" => RenderObjectKind::Fonts,
        _ => panic!("Unexpected golden object kind"),
    }
}

#[test]
fn typescript_manifest_object_and_profile_hashes_match_rust() {
    let root = fixtures();
    let golden: Value =
        serde_json::from_slice(&fs::read(root.join("golden.json")).unwrap()).unwrap();
    for (manifest_name, hash_key, profile_key, scene_name, profile_name) in [
        (
            "manifest.json",
            "packageHash",
            "profileHash",
            "scene.bin",
            "sunton-profile.json",
        ),
        (
            "waveshare-manifest.json",
            "wavesharePackageHash",
            "waveshareProfileHash",
            "waveshare-scene.bin",
            "waveshare-profile.json",
        ),
    ] {
        let bytes = fs::read(root.join(manifest_name)).unwrap();
        let manifest: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json_canonicalizer::to_vec(&manifest).unwrap(), bytes);
        assert_eq!(digest(&bytes), golden[hash_key]);
        assert_eq!(manifest["targetProfileHash"], golden[profile_key]);
        let profile_bytes = fs::read(root.join(profile_name)).unwrap();
        let profile: Value = serde_json::from_slice(&profile_bytes).unwrap();
        assert_eq!(
            serde_json_canonicalizer::to_vec(&profile).unwrap(),
            profile_bytes
        );
        assert_eq!(digest(&profile_bytes), golden[profile_key]);
        let mut object_bytes = 0;
        for descriptor in manifest["objects"].as_array().unwrap() {
            let name = descriptor["kind"].as_str().unwrap();
            let file = if name == "scene" {
                scene_name.to_owned()
            } else {
                format!("{name}.bin")
            };
            let bytes = fs::read(root.join(file)).unwrap();
            assert_eq!(bytes.len() as u64, descriptor["bytes"].as_u64().unwrap());
            assert_eq!(digest(&bytes), descriptor["sha256"]);
            assert!(!inspect_render_object(kind(name), &bytes, bytes.len())
                .unwrap()
                .is_empty());
            object_bytes += bytes.len() as u64;
        }
        assert_eq!(
            object_bytes,
            manifest["resources"]["objectBytes"].as_u64().unwrap()
        );
        assert_eq!(
            manifest["semanticDescriptorHash"],
            manifest["objects"][0]["sha256"]
        );
    }
}

#[test]
fn every_rust_record_layout_matches_the_typescript_abi() {
    let abi: Value =
        serde_json::from_slice(&fs::read(fixtures().join("abi.json")).unwrap()).unwrap();
    assert_eq!(abi["headerBytes"], 32);
    assert_eq!(abi["directoryBytes"], 16);
    for (name, magic) in [
        ("semantics", b"OSGC"),
        ("scene", b"OSGS"),
        ("images", b"OSGI"),
        ("fonts", b"OSGF"),
    ] {
        let mut layouts: Vec<_> = abi["recordBytes"][name]
            .as_object()
            .unwrap()
            .iter()
            .map(|(id, size)| (id.parse::<u16>().unwrap(), size.as_u64().unwrap() as u16))
            .collect();
        layouts.sort_unstable();
        let mut bytes = vec![0; 32 + layouts.len() * 16];
        bytes[..4].copy_from_slice(magic);
        bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&32u16.to_le_bytes());
        bytes[12..16].copy_from_slice(&32u32.to_le_bytes());
        bytes[16..18].copy_from_slice(&(layouts.len() as u16).to_le_bytes());
        for (index, (id, size)) in layouts.iter().copied().enumerate() {
            let offset = (bytes.len() + 3) & !3;
            let base = 32 + index * 16;
            bytes.resize(offset + usize::from(size), 0);
            bytes[base..base + 2].copy_from_slice(&id.to_le_bytes());
            bytes[base + 2..base + 4].copy_from_slice(&size.to_le_bytes());
            bytes[base + 4..base + 8].copy_from_slice(&(offset as u32).to_le_bytes());
            bytes[base + 8..base + 12].copy_from_slice(&1u32.to_le_bytes());
            bytes[base + 12..base + 16].copy_from_slice(&u32::from(size).to_le_bytes());
        }
        let length = bytes.len() as u32;
        bytes[8..12].copy_from_slice(&length.to_le_bytes());
        assert_eq!(
            inspect_render_object(kind(name), &bytes, bytes.len())
                .unwrap()
                .len(),
            layouts.len()
        );
    }
}
