//! Validated scheduler helper assets shipped inside the macOS application bundle.

use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, os::unix::fs::PermissionsExt, path::Path};
use thiserror::Error;

const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_BUILD_INFO_BYTES: u64 = 64 * 1024;
const MAX_HELPER_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_HELPER_BYTES: u64 = 128 * 1024 * 1024;
const PROTOCOL_VERSION: u64 = 1;

#[derive(Debug, Error)]
pub(crate) enum BundleError {
    #[error("scheduler helper bundle resources are unavailable")]
    NotFound,
    #[error("scheduler helper bundle metadata is invalid")]
    InvalidMetadata,
    #[error("scheduler helper bundle file is unavailable")]
    FileUnavailable,
    #[error("scheduler helper bundle does not match its SHA-256 receipt")]
    DigestMismatch,
    #[error("scheduler helper binary format or architecture is invalid")]
    InvalidBinary,
}

#[derive(Clone, Debug)]
pub(crate) struct HelperAsset {
    pub(crate) sha256: String,
    bytes: Vec<u8>,
}

impl HelperAsset {
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

#[derive(Clone, Debug)]
pub(crate) struct BundleAssets {
    helpers: BTreeMap<String, HelperAsset>,
}

impl BundleAssets {
    pub(crate) fn discover() -> Result<Self, BundleError> {
        let executable = std::env::current_exe().map_err(|_| BundleError::NotFound)?;
        for ancestor in executable.ancestors() {
            let candidate = ancestor.join("Resources/remote-scheduler");
            if candidate.join("manifest.json").is_file() {
                return Self::load(&candidate);
            }
        }
        Err(BundleError::NotFound)
    }

    pub(crate) fn load(root: &Path) -> Result<Self, BundleError> {
        let root = root.canonicalize().map_err(|_| BundleError::NotFound)?;
        let build_info_path = root
            .parent()
            .map(|resources| resources.join("build-info.json"))
            .ok_or(BundleError::InvalidMetadata)?;
        let build_info = read_json_file(&build_info_path, MAX_BUILD_INFO_BYTES)?;
        let source_sha = build_info
            .get("source_sha")
            .and_then(Value::as_str)
            .filter(|value| is_lower_hex(value, 40))
            .ok_or(BundleError::InvalidMetadata)?;
        let build_receipt = build_info
            .get("scheduler_manifest")
            .and_then(Value::as_object)
            .ok_or(BundleError::InvalidMetadata)?;
        if build_receipt.get("path").and_then(Value::as_str)
            != Some("remote-scheduler/manifest.json")
            || build_receipt.get("schema_version").and_then(Value::as_u64) != Some(1)
            || build_receipt
                .get("scheduler_protocol_version")
                .and_then(Value::as_u64)
                != Some(PROTOCOL_VERSION)
        {
            return Err(BundleError::InvalidMetadata);
        }
        let declared_keys = build_receipt
            .get("helpers")
            .and_then(Value::as_array)
            .ok_or(BundleError::InvalidMetadata)?
            .iter()
            .map(|value| value.as_str().ok_or(BundleError::InvalidMetadata))
            .collect::<Result<std::collections::BTreeSet<_>, _>>()?;

        let manifest_path = root.join("manifest.json");
        let manifest = read_json_file(&manifest_path, MAX_MANIFEST_BYTES)?;
        let manifest = manifest.as_object().ok_or(BundleError::InvalidMetadata)?;
        if manifest.len() != 4
            || manifest.get("schema_version").and_then(Value::as_u64) != Some(1)
            || manifest
                .get("scheduler_protocol_version")
                .and_then(Value::as_u64)
                != Some(PROTOCOL_VERSION)
            || manifest.get("source_sha").and_then(Value::as_str) != Some(source_sha)
        {
            return Err(BundleError::InvalidMetadata);
        }
        let entries = manifest
            .get("helpers")
            .and_then(Value::as_object)
            .filter(|entries| !entries.is_empty())
            .ok_or(BundleError::InvalidMetadata)?;
        if entries
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>()
            != declared_keys
        {
            return Err(BundleError::InvalidMetadata);
        }

        let mut helpers = BTreeMap::new();
        let mut total_bytes = 0_u64;
        for (key, entry) in entries {
            let (os, arch, format, linkage, machine) = match key.as_str() {
                "linux-x86_64" => ("linux", "x86_64", "elf", "static-musl", Some(62_u16)),
                "linux-aarch64" => ("linux", "aarch64", "elf", "static-musl", Some(183_u16)),
                "darwin-universal" => ("darwin", "universal", "mach-o-universal", "system", None),
                _ => return Err(BundleError::InvalidMetadata),
            };
            let entry = entry.as_object().ok_or(BundleError::InvalidMetadata)?;
            if entry.len() != 6
                || entry.get("path").and_then(Value::as_str)
                    != Some(format!("{key}/zmux-scheduler").as_str())
                || entry.get("os").and_then(Value::as_str) != Some(os)
                || entry.get("arch").and_then(Value::as_str) != Some(arch)
                || entry.get("format").and_then(Value::as_str) != Some(format)
                || entry.get("linkage").and_then(Value::as_str) != Some(linkage)
            {
                return Err(BundleError::InvalidMetadata);
            }
            let digest = entry
                .get("sha256")
                .and_then(Value::as_str)
                .filter(|value| is_lower_hex(value, 64))
                .ok_or(BundleError::InvalidMetadata)?;
            let relative = Path::new(key).join("zmux-scheduler");
            let path = root.join(&relative);
            let metadata = fs::symlink_metadata(&path).map_err(|_| BundleError::FileUnavailable)?;
            if !metadata.file_type().is_file()
                || metadata.permissions().mode() & 0o111 == 0
                || metadata.len() == 0
                || metadata.len() > MAX_HELPER_BYTES
            {
                return Err(BundleError::InvalidBinary);
            }
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .filter(|total| *total <= MAX_TOTAL_HELPER_BYTES)
                .ok_or(BundleError::InvalidBinary)?;
            let resolved = path
                .canonicalize()
                .map_err(|_| BundleError::FileUnavailable)?;
            if !resolved.starts_with(&root) {
                return Err(BundleError::InvalidMetadata);
            }
            let bytes = fs::read(&path).map_err(|_| BundleError::FileUnavailable)?;
            if sha256(&bytes) != digest {
                return Err(BundleError::DigestMismatch);
            }
            if let Some(machine) = machine {
                verify_static_elf(&bytes, machine)?;
            } else {
                verify_universal_mach_o(&bytes)?;
            }
            helpers.insert(
                key.clone(),
                HelperAsset {
                    sha256: digest.to_owned(),
                    bytes,
                },
            );
        }
        Ok(Self { helpers })
    }

    pub(crate) fn helper_for(&self, os: &str, arch: &str) -> Option<&HelperAsset> {
        match (os, arch) {
            ("linux", "x86_64") => self.helpers.get("linux-x86_64"),
            ("linux", "aarch64") => self.helpers.get("linux-aarch64"),
            ("darwin", "x86_64" | "aarch64") => self.helpers.get("darwin-universal"),
            _ => None,
        }
    }

    pub(crate) fn helper_for_key(&self, key: &str) -> Option<&HelperAsset> {
        self.helpers.get(key)
    }
}

fn read_json_file(path: &Path, limit: u64) -> Result<Value, BundleError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| BundleError::FileUnavailable)?;
    if !metadata.file_type().is_file() || metadata.len() > limit {
        return Err(BundleError::InvalidMetadata);
    }
    let bytes = fs::read(path).map_err(|_| BundleError::FileUnavailable)?;
    serde_json::from_slice(&bytes).map_err(|_| BundleError::InvalidMetadata)
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn verify_static_elf(bytes: &[u8], expected_machine: u16) -> Result<(), BundleError> {
    if bytes.len() < 64
        || &bytes[..4] != b"\x7fELF"
        || bytes[4] != 2
        || bytes[5] != 1
        || !matches!(u16_at(bytes, 16)?, 2 | 3)
        || u16_at(bytes, 18)? != expected_machine
        || u32_at(bytes, 20)? != 1
        || u16_at(bytes, 54)? != 56
    {
        return Err(BundleError::InvalidBinary);
    }
    let phoff = usize::try_from(u64_at(bytes, 32)?).map_err(|_| BundleError::InvalidBinary)?;
    let phentsize = usize::from(u16_at(bytes, 54)?);
    let phnum = usize::from(u16_at(bytes, 56)?);
    let phend = phoff
        .checked_add(
            phentsize
                .checked_mul(phnum)
                .ok_or(BundleError::InvalidBinary)?,
        )
        .filter(|end| *end <= bytes.len())
        .ok_or(BundleError::InvalidBinary)?;
    let mut dynamic_ranges = Vec::new();
    for offset in (phoff..phend).step_by(phentsize) {
        let kind = u32_at(bytes, offset)?;
        if kind == 3 {
            return Err(BundleError::InvalidBinary); // PT_INTERP
        }
        if kind == 2 {
            let start = usize::try_from(u64_at(bytes, offset + 8)?)
                .map_err(|_| BundleError::InvalidBinary)?;
            let size = usize::try_from(u64_at(bytes, offset + 32)?)
                .map_err(|_| BundleError::InvalidBinary)?;
            let end = start
                .checked_add(size)
                .filter(|end| *end <= bytes.len())
                .ok_or(BundleError::InvalidBinary)?;
            if size % 16 != 0 {
                return Err(BundleError::InvalidBinary);
            }
            dynamic_ranges.push(start..end);
        }
    }
    for range in dynamic_ranges {
        for entry in (range.start..range.end).step_by(16) {
            let tag = i64::from_le_bytes(
                bytes[entry..entry + 8]
                    .try_into()
                    .map_err(|_| BundleError::InvalidBinary)?,
            );
            if tag == 1 {
                return Err(BundleError::InvalidBinary); // DT_NEEDED
            }
            if tag == 0 {
                break;
            }
        }
    }
    Ok(())
}

fn verify_universal_mach_o(bytes: &[u8]) -> Result<(), BundleError> {
    if bytes.len() < 8 {
        return Err(BundleError::InvalidBinary);
    }
    let magic = u32::from_be_bytes(
        bytes[..4]
            .try_into()
            .map_err(|_| BundleError::InvalidBinary)?,
    );
    let (count, entry_size, is_64) = match magic {
        0xcafe_babe => (be_u32(bytes, 4)?, 20_usize, false),
        0xcafe_babf => (be_u32(bytes, 4)?, 32_usize, true),
        _ => return Err(BundleError::InvalidBinary),
    };
    if count == 0 || count > 16 {
        return Err(BundleError::InvalidBinary);
    }
    let end = 8_usize
        .checked_add(usize::try_from(count).map_err(|_| BundleError::InvalidBinary)? * entry_size)
        .filter(|end| *end <= bytes.len())
        .ok_or(BundleError::InvalidBinary)?;
    let (mut x86_64, mut arm64) = (false, false);
    for entry in (8..end).step_by(entry_size) {
        let cpu = be_u32(bytes, entry)?;
        let (offset, size) = if is_64 {
            (
                usize::try_from(be_u64(bytes, entry + 8)?)
                    .map_err(|_| BundleError::InvalidBinary)?,
                usize::try_from(be_u64(bytes, entry + 16)?)
                    .map_err(|_| BundleError::InvalidBinary)?,
            )
        } else {
            (
                be_u32(bytes, entry + 8)? as usize,
                be_u32(bytes, entry + 12)? as usize,
            )
        };
        let slice_end = offset
            .checked_add(size)
            .filter(|value| *value <= bytes.len() && offset >= end && size >= 32)
            .ok_or(BundleError::InvalidBinary)?;
        let slice = &bytes[offset..slice_end];
        let little_endian = u32::from_le_bytes(
            slice[..4]
                .try_into()
                .map_err(|_| BundleError::InvalidBinary)?,
        ) == 0xfeed_facf;
        let big_endian = u32::from_be_bytes(
            slice[..4]
                .try_into()
                .map_err(|_| BundleError::InvalidBinary)?,
        ) == 0xfeed_facf;
        if !little_endian && !big_endian {
            return Err(BundleError::InvalidBinary);
        }
        let slice_cpu = if little_endian {
            u32::from_le_bytes(
                slice[4..8]
                    .try_into()
                    .map_err(|_| BundleError::InvalidBinary)?,
            )
        } else {
            u32::from_be_bytes(
                slice[4..8]
                    .try_into()
                    .map_err(|_| BundleError::InvalidBinary)?,
            )
        };
        if slice_cpu != cpu {
            return Err(BundleError::InvalidBinary);
        }
        match cpu {
            0x0100_0007 => x86_64 = true,
            0x0100_000c => arm64 = true,
            _ => {}
        }
    }
    if x86_64 && arm64 {
        Ok(())
    } else {
        Err(BundleError::InvalidBinary)
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, BundleError> {
    bytes
        .get(offset..offset + 2)
        .and_then(|value| value.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(BundleError::InvalidBinary)
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, BundleError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_le_bytes)
        .ok_or(BundleError::InvalidBinary)
}

fn u64_at(bytes: &[u8], offset: usize) -> Result<u64, BundleError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_le_bytes)
        .ok_or(BundleError::InvalidBinary)
}

fn be_u32(bytes: &[u8], offset: usize) -> Result<u32, BundleError> {
    bytes
        .get(offset..offset + 4)
        .and_then(|value| value.try_into().ok())
        .map(u32::from_be_bytes)
        .ok_or(BundleError::InvalidBinary)
}

fn be_u64(bytes: &[u8], offset: usize) -> Result<u64, BundleError> {
    bytes
        .get(offset..offset + 8)
        .and_then(|value| value.try_into().ok())
        .map(u64::from_be_bytes)
        .ok_or(BundleError::InvalidBinary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use uuid::Uuid;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("zmux-scheduler-bundle-{}", Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create scheduler bundle fixture root");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn executable(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().expect("asset parent")).expect("create asset directory");
        fs::write(path, bytes).expect("write asset fixture");
        let mut permissions = fs::metadata(path).expect("inspect asset").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("make asset executable");
    }

    fn universal_mach_o() -> Vec<u8> {
        let mut bytes = vec![0_u8; 112];
        bytes[..4].copy_from_slice(&0xcafe_babe_u32.to_be_bytes());
        bytes[4..8].copy_from_slice(&2_u32.to_be_bytes());
        bytes[8..12].copy_from_slice(&0x0100_0007_u32.to_be_bytes());
        bytes[16..20].copy_from_slice(&48_u32.to_be_bytes());
        bytes[20..24].copy_from_slice(&32_u32.to_be_bytes());
        bytes[28..32].copy_from_slice(&0x0100_000c_u32.to_be_bytes());
        bytes[36..40].copy_from_slice(&80_u32.to_be_bytes());
        bytes[40..44].copy_from_slice(&32_u32.to_be_bytes());
        bytes[48..52].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
        bytes[52..56].copy_from_slice(&0x0100_0007_u32.to_le_bytes());
        bytes[80..84].copy_from_slice(&0xfeed_facf_u32.to_le_bytes());
        bytes[84..88].copy_from_slice(&0x0100_000c_u32.to_le_bytes());
        bytes
    }

    fn write_test_bundle(resources: &Path) -> PathBuf {
        let root = resources.join("remote-scheduler");
        let linux = elf64(62, &[7, 8, 0], false);
        let darwin = universal_mach_o();
        executable(&root.join("linux-x86_64/zmux-scheduler"), &linux);
        executable(&root.join("darwin-universal/zmux-scheduler"), &darwin);
        let helpers = serde_json::json!({
            "linux-x86_64": {
                "path": "linux-x86_64/zmux-scheduler",
                "sha256": sha256(&linux),
                "os": "linux",
                "arch": "x86_64",
                "format": "elf",
                "linkage": "static-musl"
            },
            "darwin-universal": {
                "path": "darwin-universal/zmux-scheduler",
                "sha256": sha256(&darwin),
                "os": "darwin",
                "arch": "universal",
                "format": "mach-o-universal",
                "linkage": "system"
            }
        });
        fs::write(
            root.join("manifest.json"),
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "source_sha": "a".repeat(40),
                "scheduler_protocol_version": 1,
                "helpers": helpers
            }))
            .expect("serialize scheduler manifest"),
        )
        .expect("write scheduler manifest");
        fs::write(
            resources.join("build-info.json"),
            serde_json::to_vec(&serde_json::json!({
                "source_sha": "a".repeat(40),
                "scheduler_manifest": {
                    "path": "remote-scheduler/manifest.json",
                    "schema_version": 1,
                    "scheduler_protocol_version": 1,
                    "helpers": ["darwin-universal", "linux-x86_64"]
                }
            }))
            .expect("serialize build receipt"),
        )
        .expect("write build receipt");
        root
    }

    fn elf64(machine: u16, dynamic_tags: &[i64], interp: bool) -> Vec<u8> {
        let dynamic_size = dynamic_tags.len() * 16;
        let phnum = 1 + usize::from(interp);
        let phoff = 64;
        let dynamic_offset = phoff + phnum * 56;
        let mut bytes = vec![0_u8; dynamic_offset + dynamic_size];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[6] = 1;
        bytes[16..18].copy_from_slice(&3_u16.to_le_bytes()); // ET_DYN static PIE
        bytes[18..20].copy_from_slice(&machine.to_le_bytes());
        bytes[20..24].copy_from_slice(&1_u32.to_le_bytes());
        bytes[32..40].copy_from_slice(&(phoff as u64).to_le_bytes());
        bytes[52..54].copy_from_slice(&64_u16.to_le_bytes());
        bytes[54..56].copy_from_slice(&56_u16.to_le_bytes());
        bytes[56..58].copy_from_slice(&(phnum as u16).to_le_bytes());
        let mut program = phoff;
        if interp {
            bytes[program..program + 4].copy_from_slice(&3_u32.to_le_bytes());
            bytes[program + 8..program + 16]
                .copy_from_slice(&(dynamic_offset as u64).to_le_bytes());
            bytes[program + 32..program + 40].copy_from_slice(&1_u64.to_le_bytes());
            program += 56;
        }
        bytes[program..program + 4].copy_from_slice(&2_u32.to_le_bytes()); // PT_DYNAMIC
        bytes[program + 8..program + 16].copy_from_slice(&(dynamic_offset as u64).to_le_bytes());
        bytes[program + 32..program + 40].copy_from_slice(&(dynamic_size as u64).to_le_bytes());
        for (index, tag) in dynamic_tags.iter().enumerate() {
            let offset = dynamic_offset + index * 16;
            bytes[offset..offset + 8].copy_from_slice(&tag.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn accepts_static_pie_with_dynamic_relocations_but_no_interpreter_or_needed_library() {
        let valid = elf64(62, &[7, 8, 0], false);
        assert!(verify_static_elf(&valid, 62).is_ok());
        let with_interpreter = elf64(62, &[0], true);
        assert!(matches!(
            verify_static_elf(&with_interpreter, 62),
            Err(BundleError::InvalidBinary)
        ));
        let with_needed = elf64(62, &[1, 0], false);
        assert!(matches!(
            verify_static_elf(&with_needed, 62),
            Err(BundleError::InvalidBinary)
        ));
    }

    #[test]
    fn validates_universal_mach_o_architecture_table() {
        let mut bytes = universal_mach_o();
        assert!(verify_universal_mach_o(&bytes).is_ok());
        bytes[84..88].copy_from_slice(&12_u32.to_le_bytes());
        assert!(matches!(
            verify_universal_mach_o(&bytes),
            Err(BundleError::InvalidBinary)
        ));
    }

    #[test]
    fn loads_manifest_from_the_packaged_contents_resources_layout() {
        let scratch = Scratch::new();
        let resources = scratch.0.join("Contents/Resources");
        fs::create_dir_all(&resources).expect("create app Contents/Resources");
        let root = write_test_bundle(&resources);
        let assets = BundleAssets::load(&root).expect("load packaged scheduler resources");
        assert!(assets.helper_for("linux", "x86_64").is_some());
        assert!(assets.helper_for("linux", "aarch64").is_none());
        assert!(assets.helper_for("darwin", "aarch64").is_some());

        fs::write(root.join("linux-x86_64/zmux-scheduler"), b"changed")
            .expect("tamper fixture after validated load");
        assert!(matches!(
            BundleAssets::load(&root),
            Err(BundleError::DigestMismatch)
        ));
    }
}
