//! Bounded observations from one published snapshot, without refreshing its owners.
use crate::{Frame, Snapshot};
use serde_json::{Value, json};
use zmux_workspace_core::{SurfaceKind, targets::TargetKind};

pub const DEFAULT_CURRENT_LIMIT: usize = 100;
pub const MAX_CURRENT_LIMIT: usize = 200;
pub const MAX_CURRENT_REPORT_BYTES: usize = 512 * 1024;

pub(crate) fn default_limit() -> usize {
    DEFAULT_CURRENT_LIMIT
}

fn identity(snapshot: &Snapshot, kind: TargetKind, id: u64) -> Value {
    json!({
        "id": id,
        "uuid": snapshot.model.object_uuid(kind, id),
        "reference": format!("{kind}:{id}"),
    })
}

fn session(frame: Option<&Frame>, scope: &str) -> Value {
    let state = match frame {
        Some(frame) if frame.starting => json!("starting"),
        Some(frame) => match &frame.snapshot {
            Some(snapshot) => json!(snapshot.state),
            None if frame.failure.is_some() => json!("failed"),
            None => json!("unavailable"),
        },
        None => json!("unavailable"),
    };
    json!({
        "state": state,
        "published_local_child_pid": frame.and_then(|f| f.process_id),
        "exit_code": frame.and_then(|f| f.snapshot.as_ref()).and_then(|s| s.exit_code),
        "failure_present": frame.is_some_and(|f| f.failure.is_some()
            || f.snapshot.as_ref().is_some_and(|s| s.failure.is_some())),
        "scope": scope,
        "evidence": "desktop.control_frames",
        "pid_liveness": "not_refreshed",
    })
}

pub(crate) fn current(snapshot: &Snapshot, limit: usize) -> Result<Value, (&'static str, String)> {
    if !(1..=MAX_CURRENT_LIMIT).contains(&limit) {
        return Err((
            "invalid_current_limit",
            "current limit must be 1–200".into(),
        ));
    }
    let model = &snapshot.model;
    let total: usize = model
        .workspaces
        .iter()
        .flat_map(|w| w.root.panes())
        .map(|p| p.surfaces.len())
        .sum();
    let mut items = Vec::with_capacity(total.min(limit));
    let mut workspaces = Vec::with_capacity(model.workspaces.len());
    for window in &model.windows {
        for workspace_id in &window.workspace_ids {
            let Some(workspace) = model.workspaces.iter().find(|w| w.id == *workspace_id) else {
                return Err((
                    "current_owner_unavailable",
                    "Published window membership has no workspace owner".into(),
                ));
            };
            let panes = workspace.root.panes();
            let metadata = snapshot
                .workspace_metadata
                .iter()
                .find(|m| m.workspace_id == workspace.id);
            let unread: Vec<_> = snapshot
                .notifications
                .iter()
                .filter(|n| {
                    !n.read
                        && n.identity_namespace == model.identity_namespace
                        && n.workspace_id == workspace.id
                        && n.surface_id.is_none_or(|id| {
                            panes.iter().any(|p| p.surfaces.iter().any(|s| s.id == id))
                        })
                })
                .collect();
            workspaces.push(json!({
                "workspace": identity(snapshot, TargetKind::Workspace, workspace.id.0),
                "window": identity(snapshot, TargetKind::Window, window.id.0),
                "metadata": {
                    "status_entries": metadata.map_or(0, |m| m.statuses.len()),
                    "progress_value": metadata.and_then(|m| m.progress.as_ref()).map(|p| p.value),
                    "retained_logs": metadata.map_or(0, |m| m.logs.len()),
                    "logs_dropped": metadata.map_or(0, |m| m.logs_dropped),
                    "evidence": "desktop.workspace_metadata",
                },
                "unread_notifications": unread.len(),
                "attention_evidence": "desktop.notifications_matching_live_owner",
            }));
            for pane in panes {
                for surface in &pane.surfaces {
                    if items.len() == limit {
                        break;
                    }
                    let frame = surface
                        .session_id
                        .and_then(|id| snapshot.control_frames.iter().find(|f| f.session_id == id));
                    items.push(json!({
                        "window": identity(snapshot, TargetKind::Window, window.id.0),
                        "workspace": identity(snapshot, TargetKind::Workspace, workspace.id.0),
                        "pane": identity(snapshot, TargetKind::Pane, pane.id.0),
                        "surface": identity(snapshot, TargetKind::Surface, surface.id.0),
                        "kind": surface.kind,
                        "selected": pane.selected_surface == surface.id,
                        "logical_focus": model.focused_window == Some(window.id)
                            && model.focused_workspace == Some(workspace.id)
                            && workspace.focused_pane == pane.id
                            && pane.selected_surface == surface.id,
                        "session_id": surface.session_id,
                        "session": if surface.kind == SurfaceKind::Terminal {
                            session(frame, if surface.remote.is_some() {
                                "local_ssh_transport_child"
                            } else if surface.local_tmux.is_some() {
                                "local_tmux_client"
                            } else { "local_session_child" })
                        } else { Value::Null },
                        "activity": { "state": "unknown", "reason": "published lifecycle does not establish active work" },
                        "unread_notifications": unread.iter().filter(|n| n.surface_id == Some(surface.id)).count(),
                    }));
                }
            }
        }
    }
    let report = json!({
        "schema_version": 1,
        "source": "published_desktop_snapshot",
        "publication_revision": snapshot.revision,
        "model_revision": model.revision,
        "identity_namespace": model.identity_namespace,
        "freshness": { "kind": "cached_publication", "observed_at": null, "owners_refreshed": false },
        "focused_window": model.focused_window,
        "focused_workspace": model.focused_workspace,
        "desktop_closed": snapshot.closed,
        "limit": limit,
        "total_surfaces": total,
        "returned_surfaces": items.len(),
        "truncated": items.len() < total,
        "workspaces": workspaces,
        "items": items,
        "limitations": [
            "Active tasks, commands, human obligations and browser activity are unknown",
            "Child lifecycle and PID are cached local observations, not process or remote liveness",
            "Cloud machines, resource usage, transcripts and external repositories are not inspected",
            "Metadata and unread notifications are observations, not permission to act",
        ],
    });
    let bytes = serde_json::to_vec(&report).map_err(|e| ("current_encoding", e.to_string()))?;
    if bytes.len() > MAX_CURRENT_REPORT_BYTES {
        return Err((
            "current_capacity",
            "Published current report exceeds 512 KiB".into(),
        ));
    }
    Ok(report)
}
