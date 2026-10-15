use std::ffi::c_void;

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct RawColor {
    pub kind: u8,
    pub index: u8,
    pub r: u8,
    pub g: u8,
    pub b: u8,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub struct RawCell {
    pub text_offset: u32,
    pub text_length: u32,
    pub row: u16,
    pub column: u16,
    pub width: u8,
    pub foreground: RawColor,
    pub background: RawColor,
    pub bold: u8,
    pub italic: u8,
    pub underline: u8,
    pub inverse: u8,
}
#[derive(Default)]
#[repr(C)]
pub struct RawFrame {
    pub version: u32,
    pub unsupported: u32,
    pub history_rows: u64,
    pub scroll_total: u64,
    pub scroll_offset: u64,
    pub scroll_len: u64,
    pub cells_length: usize,
    pub text_length: usize,
    pub rows: u16,
    pub cols: u16,
    pub cursor_x: u16,
    pub cursor_y: u16,
    pub pending_wrap: u8,
    pub cursor_visible: u8,
    pub application_cursor: u8,
    pub bracketed_paste: u8,
    pub alternate: u8,
}

#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct Batch {
    pub operation_id: u64,
    pub text_length: usize,
    pub event_length: usize,
    pub reply_length: usize,
    pub x: u16,
    pub y: u16,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
pub(crate) struct Event {
    pub kind: u32,
    pub offset: u32,
    pub length: u32,
}

unsafe extern "C" {
    pub(crate) fn zmux_vt_new(
        cols: u16,
        rows: u16,
        history: usize,
        engine: usize,
        general: usize,
        pages: usize,
    ) -> *mut c_void;
    pub(crate) fn metadata_integrated(p: *mut c_void) -> *mut c_void;
    pub(crate) fn metadata_copy(
        p: *mut c_void,
        records: *mut crate::metadata::MetaRecord,
        size: usize,
        capacity: usize,
        count: *mut usize,
        bytes: *mut u8,
        byte_capacity: usize,
        length: *mut usize,
    ) -> i32;
    pub(crate) fn metadata_commit(p: *mut c_void, operation: u64) -> i32;
    pub(crate) fn metadata_free(p: *mut c_void);
    pub(crate) fn integrated_feed(p: *mut c_void, bytes: *const u8, length: usize) -> i32;
    pub(crate) fn integrated_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    pub(crate) fn integrated_scroll(p: *mut c_void, lines: i32) -> i32;
    pub(crate) fn integrated_abort(p: *mut c_void);
    pub(crate) fn zmux_vt_export(
        p: *mut c_void,
        batch: *mut Batch,
        batch_size: usize,
        frame: *mut RawFrame,
        frame_size: usize,
        cells: *mut RawCell,
        cell_size: usize,
        cell_capacity: usize,
        text: *mut u8,
        text_capacity: usize,
        events: *mut Event,
        event_size: usize,
        event_capacity: usize,
        replies: *mut u8,
        reply_capacity: usize,
    ) -> i32;
}
