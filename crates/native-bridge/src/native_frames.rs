//! Bounded native presentation deltas; full snapshots remain available to CLI/API
//! readers. A missed/unknown baseline always produces full coherent frames.
use serde::Serialize;
use std::{collections::VecDeque, sync::Arc};
use zmux_desktop_runtime::{Frame, Snapshot};
use zmux_session_runtime::{Snapshot as TerminalSnapshot, State};
use zmux_terminal_adapter::{LinkSpan, PointerTracking, RowCopy, Run, Size, Viewport};

pub(crate) struct Baseline {
    pub revision: u64,
    pub namespace: uuid::Uuid,
    pub frames: Vec<Frame>,
}
impl Baseline {
    pub(crate) fn new(snapshot: &Snapshot) -> Self {
        Self {
            revision: snapshot.revision,
            namespace: snapshot.model.identity_namespace,
            frames: snapshot.frames.clone(),
        }
    }
}
pub(crate) type History = VecDeque<Baseline>;
pub(crate) const BASELINES: usize = 2;

#[derive(Serialize)]
struct Envelope<'a> {
    schema_version: u8,
    base_revision: Option<u64>,
    snapshot: Snapshot,
    frame_packets: Vec<Packet<'a>>,
}
#[derive(Serialize)]
struct Packet<'a> {
    frame: WireFrame<'a>,
    unchanged: bool,
    changed_rows: Option<Vec<usize>>,
    contents_start: usize,
    contents_end: usize,
}
#[derive(Serialize)]
struct WireFrame<'a> {
    session_id: zmux_workspace_core::SessionId,
    process_id: Option<u32>,
    starting: bool,
    reconnectable: bool,
    failure: &'a Option<String>,
    snapshot: Option<WireTerminal<'a>>,
}
#[derive(Serialize)]
struct WireTerminal<'a> {
    state: State,
    bytes_read: u64,
    exit_code: Option<u32>,
    failure: &'a Option<String>,
    viewport: WireViewport<'a>,
}
/// Same native viewport metadata contract. Empty unchanged row slots are merged
/// before presentation; `contents` is a UTF-8 replacement span when rows is Some.
#[derive(Serialize)]
struct WireViewport<'a> {
    viewport_generation: u64,
    links: &'a [LinkSpan],
    size: Size,
    title: &'a str,
    working_directory: &'a Option<String>,
    contents: &'a str,
    lines: Vec<&'a [Run]>,
    row_copy: &'a Option<Vec<RowCopy>>,
    cursor_row: u16,
    cursor_col: u16,
    pointer_tracking: &'a Option<PointerTracking>,
    application_cursor: bool,
    bracketed_paste: bool,
    hide_cursor: bool,
    scrollback_offset: usize,
    scrollback_lines: usize,
    alternate_screen: bool,
}

impl<'a> WireViewport<'a> {
    fn new(
        current: &'a Viewport,
        previous: Option<&Viewport>,
    ) -> (Self, Option<Vec<usize>>, usize, usize) {
        let previous = previous
            .filter(|old| old.size == current.size && old.lines.len() == current.lines.len());
        let mut changed = Vec::new();
        let lines = current
            .lines
            .iter()
            .enumerate()
            .map(|(row, runs)| {
                if previous.is_some_and(|old| old.lines[row] == *runs) {
                    &[][..]
                } else {
                    changed.push(row);
                    runs.as_slice()
                }
            })
            .collect();
        let (start, end, contents) = previous.map_or((0, 0, current.contents.as_str()), |old| {
            text_patch(&old.contents, &current.contents)
        });
        (
            Self {
                viewport_generation: current.viewport_generation,
                links: &current.links,
                size: current.size,
                title: &current.title,
                working_directory: &current.working_directory,
                contents,
                lines,
                row_copy: &current.row_copy,
                cursor_row: current.cursor_row,
                cursor_col: current.cursor_col,
                pointer_tracking: &current.pointer_tracking,
                application_cursor: current.application_cursor,
                bracketed_paste: current.bracketed_paste,
                hide_cursor: current.hide_cursor,
                scrollback_offset: current.scrollback_offset,
                scrollback_lines: current.scrollback_lines,
                alternate_screen: current.alternate_screen,
            },
            previous.map(|_| changed),
            start,
            end,
        )
    }
}

fn text_patch<'a>(old: &str, new: &'a str) -> (usize, usize, &'a str) {
    let mut prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(prefix) || !new.is_char_boundary(prefix) {
        prefix -= 1;
    }
    let mut suffix = old.as_bytes()[prefix..]
        .iter()
        .rev()
        .zip(new.as_bytes()[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) || !new.is_char_boundary(new.len() - suffix) {
        suffix -= 1;
    }
    (prefix, old.len() - suffix, &new[prefix..new.len() - suffix])
}

fn packet<'a>(frame: &'a Frame, previous: Option<&Frame>) -> Packet<'a> {
    let unchanged = previous.is_some_and(|old| match (&old.snapshot, &frame.snapshot) {
        (Some(old), Some(new)) => Arc::ptr_eq(old, new),
        (None, None) => true,
        _ => false,
    });
    let mut changed_rows = None;
    let mut contents_start = 0;
    let mut contents_end = 0;
    let snapshot = if unchanged {
        None
    } else {
        frame.snapshot.as_deref().map(|snapshot| {
            let old = previous
                .and_then(|p| p.snapshot.as_deref())
                .map(|s| &s.viewport);
            let (viewport, rows, start, end) = WireViewport::new(&snapshot.viewport, old);
            changed_rows = rows;
            contents_start = start;
            contents_end = end;
            wire_terminal(snapshot, viewport)
        })
    };
    Packet {
        frame: WireFrame {
            session_id: frame.session_id,
            process_id: frame.process_id,
            starting: frame.starting,
            reconnectable: frame.reconnectable,
            failure: &frame.failure,
            snapshot,
        },
        unchanged,
        changed_rows,
        contents_start,
        contents_end,
    }
}
fn wire_terminal<'a>(s: &'a TerminalSnapshot, viewport: WireViewport<'a>) -> WireTerminal<'a> {
    WireTerminal {
        state: s.state,
        bytes_read: s.bytes_read,
        exit_code: s.exit_code,
        failure: &s.failure,
        viewport,
    }
}

pub(crate) fn encode(
    current: &Snapshot,
    previous: Option<&Baseline>,
) -> Result<Vec<u8>, serde_json::Error> {
    let mut metadata = current.clone();
    metadata.frames.clear();
    let frames = current
        .frames
        .iter()
        .map(|frame| {
            packet(
                frame,
                previous
                    .and_then(|old| old.frames.iter().find(|f| f.session_id == frame.session_id)),
            )
        })
        .collect();
    serde_json::to_vec(&Envelope {
        schema_version: 1,
        base_revision: previous.map(|s| s.revision),
        snapshot: metadata,
        frame_packets: frames,
    })
}
