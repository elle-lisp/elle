// audited: 2026-11-23
//! How much native stack the running thread has left, read as one compare
//! against the floor its VM records.
//!
//! docs/impl/vm.md

use std::cell::Cell;

/// Below this much native stack, a call does not enter compiled code.
#[cfg(any(feature = "jit", feature = "wasm", feature = "mlir"))]
pub(crate) const COMPILED_CALL_RESERVE: usize = 512 * 1024;

/// Below this much native stack, a re-entry into the interpreter halts.
pub(crate) const REENTRY_RESERVE: usize = 256 * 1024;

/// The address range of one thread's stack. The stack grows down, from `top`
/// towards `floor`.
#[derive(Clone, Copy)]
struct Bounds {
    floor: usize,
    top: usize,
}

thread_local! {
    /// The running thread's bounds, read from the platform once. The outer
    /// `None` means not read yet; the inner one means the platform has no
    /// answer.
    static BOUNDS: Cell<Option<Option<Bounds>>> = const { Cell::new(None) };
}

fn bounds() -> Option<Bounds> {
    BOUNDS.with(|cell| match cell.get() {
        Some(known) => known,
        None => {
            let read = thread_bounds();
            cell.set(Some(read));
            read
        }
    })
}

/// The current stack position: the address of a local in the caller's frame,
/// once this is inlined.
#[inline(always)]
fn position() -> usize {
    let marker = 0u8;
    std::hint::black_box(&marker) as *const u8 as usize
}

/// The bytes left between the current stack position and the bottom of the
/// thread's stack, or `None` where the platform does not report its bounds.
///
/// Also `None` when the current position lies outside the bounds the thread
/// reported: a host that runs the VM on a stack of its own making, such as a
/// coroutine's, has bounds this probe cannot see.
pub(crate) fn remaining() -> Option<usize> {
    let b = bounds()?;
    let position = position();
    (b.floor..=b.top)
        .contains(&position)
        .then(|| position - b.floor)
}

/// The lowest address of the stack of the thread that built a VM, or 0 where
/// the platform reports no bounds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StackFloor(usize);

impl StackFloor {
    /// The floor of the running thread's stack.
    pub(crate) fn of_this_thread() -> Self {
        StackFloor(bounds().map_or(0, |b| b.floor))
    }

    /// Whether less than `reserve` bytes lie between the current stack
    /// position and this floor.
    #[inline(always)]
    pub(crate) fn below(self, reserve: usize) -> bool {
        reads_low(position(), self.0, reserve)
    }
}

/// Whether `position` lies less than `reserve` bytes above `floor`. A position
/// below the floor wraps to a distance past every reserve.
#[inline(always)]
fn reads_low(position: usize, floor: usize, reserve: usize) -> bool {
    position.wrapping_sub(floor) < reserve
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn thread_bounds() -> Option<Bounds> {
    // SAFETY: `attr` is initialized by `pthread_getattr_np` before it is read
    // and destroyed exactly once; the two out-params are locals we own.
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) != 0 {
            return None;
        }
        let mut addr: *mut libc::c_void = std::ptr::null_mut();
        let mut size: libc::size_t = 0;
        let read = libc::pthread_attr_getstack(&attr, &mut addr, &mut size);
        libc::pthread_attr_destroy(&mut attr);
        (read == 0 && !addr.is_null()).then(|| Bounds {
            floor: addr as usize,
            top: addr as usize + size,
        })
    }
}

#[cfg(target_os = "macos")]
fn thread_bounds() -> Option<Bounds> {
    // SAFETY: both calls read the running thread's own attributes.
    unsafe {
        let thread = libc::pthread_self();
        let top = libc::pthread_get_stackaddr_np(thread) as usize;
        let size = libc::pthread_get_stacksize_np(thread);
        Some(Bounds {
            floor: top.checked_sub(size)?,
            top,
        })
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn thread_bounds() -> Option<Bounds> {
    None
}

#[cfg(test)]
mod tests;
