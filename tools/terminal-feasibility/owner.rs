// SPDX-License-Identifier: MIT
// Original MIT fixture. A single-threaded Rust owner never exports borrowed C pointers.
// Independent raw-API and compatibility entrypoints use different subsets.
#![allow(dead_code)]
use std::{ffi::c_void, marker::PhantomData, ptr::NonNull, rc::Rc};
unsafe extern "C" {
    fn probe_new(cols: u16, rows: u16) -> *mut c_void;
    fn probe_free(p: *mut c_void);
    fn probe_feed(p: *mut c_void, bytes: *const u8, n: usize) -> i32;
    fn probe_resize(p: *mut c_void, cols: u16, rows: u16) -> i32;
    fn probe_copy(
        p: *mut c_void,
        out: *mut u8,
        cap: usize,
        n: *mut usize,
        x: *mut u16,
        y: *mut u16,
    ) -> i32;
    fn probe_replies(p: *mut c_void, out: *mut u8, cap: usize, n: *mut usize) -> i32;
}
pub(crate) struct Engine {
    raw: NonNull<c_void>,
    _thread: PhantomData<Rc<()>>,
}
impl Drop for Engine {
    fn drop(&mut self) {
        unsafe { probe_free(self.raw.as_ptr()) }
    }
}
impl Engine {
    pub(crate) fn new(cols: u16, rows: u16) -> Self {
        Self {
            raw: NonNull::new(unsafe { probe_new(cols, rows) }).expect("create"),
            _thread: PhantomData,
        }
    }
    pub(crate) fn feed(&mut self, s: &[u8]) {
        assert_eq!(
            unsafe { probe_feed(self.raw.as_ptr(), s.as_ptr(), s.len()) },
            0
        );
    }
    pub(crate) fn resize(&mut self, cols: u16, rows: u16) {
        assert_eq!(unsafe { probe_resize(self.raw.as_ptr(), cols, rows) }, 0);
    }
    pub(crate) fn snapshot(&mut self) -> (Vec<String>, u16, u16) {
        let mut b = vec![0u8; 512 * 1024];
        let (mut n, mut x, mut y) = (0, 0, 0);
        assert_eq!(
            unsafe {
                probe_copy(
                    self.raw.as_ptr(),
                    b.as_mut_ptr(),
                    b.len(),
                    &mut n,
                    &mut x,
                    &mut y,
                )
            },
            0
        );
        assert!(n <= b.len());
        b.truncate(n);
        let text = String::from_utf8(b).expect("UTF8");
        (
            text.lines().map(|s| s.trim_end().to_owned()).collect(),
            x,
            y,
        )
    }
    pub(crate) fn replies(&mut self) -> Vec<u8> {
        let mut b = vec![0u8; 4096];
        let mut n = 0;
        assert_eq!(
            unsafe { probe_replies(self.raw.as_ptr(), b.as_mut_ptr(), b.len(), &mut n) },
            0
        );
        assert!(n <= b.len());
        b.truncate(n);
        b
    }
}

impl Engine {
    pub(crate) fn raw(&mut self) -> *mut c_void {
        self.raw.as_ptr()
    }
}
