// audited: 2026-10-21
//! Where a Mach-O process maps an address from: dyld's image list and each
//! image's segment load commands.
//!
//! docs/impl/image/blob.md
//!
//! The layouts and the four dyld calls are declared here rather than taken from
//! `libc`, which deprecates its Mach-O bindings. The Mach-O ABI fixes all of
//! them, and libSystem, which every macOS binary links, exports the calls.

use std::ffi::{c_char, CStr, OsStr};
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;

// Each layout is the whole ABI struct, so the fields this file never reads are
// declared all the same: the offsets of the ones it does read depend on them.

/// `mach_header_64` from `<mach-o/loader.h>`: the header the load commands
/// follow.
#[repr(C)]
#[allow(dead_code)]
struct MachHeader64 {
    magic: u32,
    cputype: i32,
    cpusubtype: i32,
    filetype: u32,
    ncmds: u32,
    sizeofcmds: u32,
    flags: u32,
    reserved: u32,
}

/// `load_command`: the prefix every load command begins with.
#[repr(C)]
struct LoadCommand {
    cmd: u32,
    cmdsize: u32,
}

/// `segment_command_64`: one segment, where it sits in memory before the
/// slide and where its bytes sit in the file.
#[repr(C)]
#[allow(dead_code)]
struct SegmentCommand64 {
    cmd: u32,
    cmdsize: u32,
    segname: [u8; 16],
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: i32,
    initprot: i32,
    nsects: u32,
    flags: u32,
}

/// The load command that names a 64-bit segment.
const LC_SEGMENT_64: u32 = 0x19;

extern "C" {
    fn _dyld_image_count() -> u32;
    fn _dyld_get_image_header(image_index: u32) -> *const MachHeader64;
    fn _dyld_get_image_vmaddr_slide(image_index: u32) -> isize;
    fn _dyld_get_image_name(image_index: u32) -> *const c_char;
}

/// The file that maps `len` bytes at `addr`, and the offset of `addr` in it.
///
/// Each image's load commands name its segments, and the image's slide moves
/// them to where they sit in memory. A universal binary holds the slice at an
/// offset the load commands do not show, so the offset is wrong there and the
/// header check refuses it.
pub(super) fn segment_of(addr: usize, len: usize) -> Option<(PathBuf, u64)> {
    // SAFETY: dyld answers a header and a name for every index below its
    // count, and a header's `ncmds` load commands follow it in memory, each
    // `cmdsize` bytes long.
    unsafe {
        for image in 0.._dyld_image_count() {
            let header = _dyld_get_image_header(image);
            if header.is_null() {
                continue;
            }
            let slide = _dyld_get_image_vmaddr_slide(image) as usize;
            let mut at = header.add(1) as *const u8;
            for _ in 0..(*header).ncmds {
                let command = &*(at as *const LoadCommand);
                if command.cmd == LC_SEGMENT_64 {
                    let segment = &*(at as *const SegmentCommand64);
                    let start = (segment.vmaddr as usize).wrapping_add(slide);
                    let backed = start.wrapping_add(segment.filesize as usize);
                    if addr >= start && addr + len <= backed {
                        let name = CStr::from_ptr(_dyld_get_image_name(image));
                        let path = PathBuf::from(OsStr::from_bytes(name.to_bytes()));
                        return Some((path, segment.fileoff + (addr - start) as u64));
                    }
                }
                at = at.add(command.cmdsize as usize);
            }
        }
    }
    None
}
