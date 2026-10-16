//! Trusted native file channel framing. Frame (both directions):
//! `u32 big-endian header_len | header JSON (UTF-8) | body bytes`. The body is raw, so a 2 MiB
//! text save is not inflated by JSON escaping and an image/PDF reply needs no base64.
use serde::Deserialize;
use serde_json::{Value, json};
use zmux_desktop_runtime::{
    Desktop,
    files::{FileContent, FileFailure, FileOwner},
};
use zmux_workspace_core::{FileMode, SurfaceId};

/// Request header cap, and the whole-frame cap of every operation except `save`.
pub const MAX_FILE_HEADER_BYTES: usize = 16_384;
/// Largest text a save may carry (`MAX_TEXT_BYTES` of the file service).
pub const MAX_FILE_SAVE_BODY_BYTES: usize = 2_097_152;
/// A save frame may exceed the ordinary request cap by its body, and only by its body.
pub const MAX_FILE_SAVE_FRAME_BYTES: usize = MAX_FILE_HEADER_BYTES + MAX_FILE_SAVE_BODY_BYTES;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenItem {
    pub kind: OpenKind,
    pub value: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpenKind {
    /// URL operands belong to the CLI router, not to this local-file channel.
    Path,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForceMode {
    Markdown,
}
fn default_focus() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum FileRequest {
    Attach {
        epoch: uuid::Uuid,
        surface_id: SurfaceId,
        host_id: uuid::Uuid,
    },
    Reload {
        owner: FileOwner,
        #[serde(default)]
        discard: bool,
    },
    Save {
        owner: FileOwner,
        base_generation: u64,
        #[serde(default)]
        force: bool,
    },
    Observe {
        owner: FileOwner,
        dirty: bool,
    },
    Detach {
        owner: FileOwner,
    },
    Open {
        items: Vec<OpenItem>,
        #[serde(default = "default_focus")]
        focus: bool,
        #[serde(default)]
        force_mode: Option<ForceMode>,
    },
}

/// A malformed, truncated, oversized or unknown frame: a caller bug (`INVALID_ARGUMENT`).
#[derive(Debug)]
pub struct InvalidFrame;

/// Split and validate one request frame.
pub fn parse(frame: &[u8]) -> Result<(FileRequest, &[u8]), InvalidFrame> {
    if frame.len() < 4 || frame.len() > MAX_FILE_SAVE_FRAME_BYTES {
        return Err(InvalidFrame);
    }
    let header_len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
    if header_len > MAX_FILE_HEADER_BYTES || 4 + header_len > frame.len() {
        return Err(InvalidFrame);
    }
    let (header, body) = frame[4..].split_at(header_len);
    let request: FileRequest = serde_json::from_slice(header).map_err(|_| InvalidFrame)?;
    match request {
        FileRequest::Save { .. } if body.len() > MAX_FILE_SAVE_BODY_BYTES => Err(InvalidFrame),
        FileRequest::Save { .. } => Ok((request, body)),
        _ if !body.is_empty() || frame.len() > MAX_FILE_HEADER_BYTES => Err(InvalidFrame),
        _ => Ok((request, body)),
    }
}

fn encode(header: &Value, body: &[u8]) -> Vec<u8> {
    let header = serde_json::to_vec(header).unwrap_or_else(|_| {
        br#"{"ok":false,"outcome_unknown":true,"error":{"code":"internal","message":"reply serialization failed"}}"#.to_vec()
    });
    let mut frame = Vec::with_capacity(4 + header.len() + body.len());
    frame.extend_from_slice(&(header.len() as u32).to_be_bytes());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(body);
    frame
}
fn failure_header(failure: &FileFailure) -> Value {
    let mut header = json!({
        "ok": false,
        "op_id": failure.op_id,
        "outcome_unknown": failure.outcome_unknown,
        "error": {
            "code": failure.code,
            "message": failure.message.chars().take(256).collect::<String>(),
        },
    });
    if let Some(generation) = failure.committed_generation {
        header["generation"] = json!(generation);
    }
    if let Some(conflict) = &failure.conflict {
        header["conflict"] = json!({
            "kind": conflict.kind,
            "disk_sha256": conflict.disk_sha256,
            "disk_size": conflict.disk_size,
        });
    }
    header
}
fn content_frame(content: FileContent) -> Vec<u8> {
    let header = json!({
        "ok": true,
        "op_id": content.op_id,
        "owner": content.owner,
        "mode": content.mode,
        "size": content.size,
        "generation": content.generation,
        "sha256": content.sha256,
        "mtime_ns": content.mtime_ns,
        "writable": content.writable,
        "bom": content.bom,
    });
    encode(&header, &content.body)
}

/// Run one parsed request. Blocking: the caller must be off the UI thread.
pub fn execute(desktop: &Desktop, request: FileRequest, body: &[u8]) -> Vec<u8> {
    let result: Result<Vec<u8>, FileFailure> = match request {
        FileRequest::Attach {
            epoch,
            surface_id,
            host_id,
        } => desktop
            .file_attach(epoch, surface_id, host_id)
            .map(content_frame),
        FileRequest::Reload { owner, discard } => {
            desktop.file_reload(&owner, discard).map(content_frame)
        }
        FileRequest::Save {
            owner,
            base_generation,
            force,
        } => desktop
            .file_save(&owner, base_generation, force, body.to_vec())
            .map(|receipt| {
                encode(
                    &json!({
                        "ok": true,
                        "op_id": receipt.op_id,
                        "generation": receipt.generation,
                        "sha256": receipt.sha256,
                        "size": receipt.size,
                        "mtime_ns": receipt.mtime_ns,
                    }),
                    &[],
                )
            }),
        FileRequest::Observe { owner, dirty } => desktop
            .file_observe(&owner, dirty)
            .map(|revision| encode(&json!({"ok": true, "revision": revision}), &[])),
        FileRequest::Detach { owner } => desktop
            .file_detach(&owner)
            .map(|revision| encode(&json!({"ok": true, "revision": revision}), &[])),
        FileRequest::Open {
            items,
            focus,
            force_mode,
        } => {
            let paths = items
                .into_iter()
                .map(|item| match item.kind {
                    OpenKind::Path => item.value,
                })
                .collect();
            desktop
                .file_open(
                    paths,
                    force_mode.map(|ForceMode::Markdown| FileMode::Markdown),
                    focus,
                )
                .map(|result| encode(&json!({"ok": true, "result": result}), &[]))
        }
    };
    result.unwrap_or_else(|failure| encode(&failure_header(&failure), &[]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_whose_text_is_already_on_disk_carries_the_generation_to_rebase_on() {
        let mut unsynced = FileFailure::new("sync_failed", "the directory could not be synced");
        unsynced.op_id = Some(13);
        unsynced.outcome_unknown = true;
        unsynced.committed_generation = Some(4);
        let header = failure_header(&unsynced);
        assert_eq!(header["ok"], false);
        assert_eq!(header["op_id"], 13);
        assert_eq!(header["outcome_unknown"], true);
        assert_eq!(header["error"]["code"], "sync_failed");
        assert_eq!(header["generation"], 4);

        let plain = failure_header(&FileFailure::new("conflict", "changed"));
        assert!(
            plain.get("generation").is_none(),
            "a failure that adopted nothing names no generation: {plain}"
        );
    }
}
