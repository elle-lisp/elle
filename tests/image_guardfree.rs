// audited: 2026-09-08
//! A hydrated region under `--trace=guardfree`: a freed image page keeps its
//! address, and refuses to be read.
//!
//! docs/impl/image/plan.md
//!
//! Guardfree trades memory for an exact answer. Instead of returning a freed
//! page to the pool it makes the page unreadable and keeps the mapping, so
//! the address is never handed to anything else and a use-after-free stops at
//! the dereference rather than reading a plausible successor. A file-backed
//! image page has to take the same treatment, because a pointer into a freed
//! hydration is the same defect as a pointer into a freed region.
//!
//! This file is its OWN test binary. `config::init` is process-global, the
//! guard installs a SIGSEGV handler, and every page this process frees
//! afterwards leaks on purpose.

use elle::image;
use elle::runtime::Runtime;
use elle::value::{HeapObject, Pair, Value};
use elle::SymbolTable;

fn base_page() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

/// Whether `addr`'s page is still mapped in this process. `msync` answers for
/// the mapping without reading the memory, which a guarded page would fault
/// on: it succeeds on a live mapping and on a `PROT_NONE` one, and fails with
/// `ENOMEM` on an address the kernel has taken back.
///
/// The trap: `mprotect` answers the same question and is the obvious probe,
/// but it is not a question — it makes the page inaccessible. Asking it about
/// a *live* image page leaves the region unreadable, and the free that comes
/// next walks that region's objects and dies in the cross-ref scan.
fn page_is_mapped(addr: usize) -> bool {
    let page = base_page();
    let base = addr & !(page - 1);
    unsafe { libc::msync(base as *mut libc::c_void, page, libc::MS_ASYNC) == 0 }
}

/// Whether reading one byte at `addr` is refused. The kernel reads the buffer
/// on the process's behalf, so an unreadable page comes back as `EFAULT`
/// instead of a signal — which is how a test can ask the question and live.
fn read_is_refused(addr: usize) -> bool {
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe");
    let wrote = unsafe { libc::write(fds[1], addr as *const libc::c_void, 1) };
    let err = std::io::Error::last_os_error().raw_os_error();
    unsafe {
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    wrote == -1 && err == Some(libc::EFAULT)
}

// § Test plan, "Diagnostics": under `--trace=guardfree`, a freed hydrated page
// stays mapped and inaccessible.
//
// The counter-factual is the ordinary release path, which unmaps a
// file-backed page: the address then belongs to nobody, a later `mmap` may
// take it, and a stale read finds whatever moved in. `page_is_mapped` is what
// tells the two apart — a read faults either way, but only the guarded page
// is still reserved.
#[test]
fn a_freed_hydrated_page_stays_reserved_and_unreadable() {
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("guardfree".to_string());
    elle::config::init(cfg);

    // Guarding is armed by `init_stdlib`, on the thread that runs it — the
    // same thread as the frees below.
    let mut rt = Runtime::new();

    let dir = std::env::temp_dir().join(format!("elle-image-guard-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("graph.image");

    let heap = rt.heap();
    let region = heap.new_runtime_region();
    let inner = heap.alloc_in_region(
        HeapObject::Pair(Pair::new(Value::int(2), Value::EMPTY_LIST)),
        region,
    );
    let root = heap.alloc_in_region(HeapObject::Pair(Pair::new(Value::int(1), inner)), region);
    image::dump(heap, &SymbolTable::new(), root, &path).expect("dump");

    let hydrated = image::hydrate_path(heap, &mut SymbolTable::new(), &path).expect("hydrate");
    let addr = hydrated
        .root
        .as_heap_ptr()
        .expect("the root is a heap value") as usize;
    assert!(page_is_mapped(addr), "the hydrated page is not mapped");

    heap.decref_region_if_present(hydrated.region);
    assert!(
        page_is_mapped(addr),
        "the freed image page was unmapped; guardfree must keep the address \
         reserved so nothing else can take it"
    );
    assert!(
        read_is_refused(addr),
        "the freed image page is still readable; a use-after-free would come \
         back with plausible bytes instead of faulting"
    );

    std::fs::remove_dir_all(&dir).ok();
}
