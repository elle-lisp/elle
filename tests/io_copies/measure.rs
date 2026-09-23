//! audited: 2026-09-23
//! The Rust heap bytes a stream read or write allocates per byte it moves,
//! counted on every thread, for the two io_copies binaries.
//!
//! docs/impl/io-bytes.md
//
// The gauge is every byte the global allocator hands out, on every thread, so
// a pool worker's buffer counts as much as the scheduler's. Region pages come
// from `mmap` rather than the allocator, so the value a read answers with is
// not counted. What is counted is the bytes staged in a Rust `Vec` on the way.
//
// The measurement is a SLOPE, for the reason `worker_heap.rs` gives: a whole
// `Runtime` is built and torn down inside the window, and its own cost is
// large but does not depend on the size of the file. The difference between
// two sizes is the cost per byte, and that is what a copy through a `Vec`
// shows up in.

use std::alloc::{GlobalAlloc, Layout, System};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// Counts every byte the process allocates, growth by `realloc` included.
pub struct Counting;

static ALLOCATED: AtomicU64 = AtomicU64::new(0);

// SAFETY: every method forwards to `System` unchanged; the counter is the only
// addition, and it touches no memory the allocator hands out.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATED.fetch_add(layout.size() as u64, Ordering::Relaxed);
        System.alloc_zeroed(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() {
            ALLOCATED.fetch_add((new_size - layout.size()) as u64, Ordering::Relaxed);
        }
        System.realloc(ptr, layout, new_size)
    }
}

/// The counter is process-wide, so two measurements must never overlap.
static SERIAL: Mutex<()> = Mutex::new(());

/// The smaller and the larger file every measurement reads. Eight megabytes
/// apart, so a copy through a `Vec` costs at least that much at the larger
/// size and a slope limit of a quarter leaves two megabytes for noise.
const SMALL: usize = 1 << 20;
const LARGE: usize = 9 << 20;

/// Take the runtime's configuration before anything reads it, and the ring or
/// the pool as the binary asks.
///
/// The trap: three things allocate at times of their own and swamp the gauge.
/// The JIT and MLIR compile in the background. The stdlib disk cache is written
/// by one runtime and read by the next, which moved a reading by hundreds of
/// megabytes and made it negative. All three are off here.
pub fn configure(no_uring: bool) {
    elle::config::init(elle::config::Config {
        jit: elle::config::JitPolicy::Off,
        mlir: elle::config::MlirPolicy::Off,
        no_uring,
        cache: None,
        ..Default::default()
    });
}

/// A scratch directory under the platform temp root, gone when this drops.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "elle-io-copies-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("create the scratch directory");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// One shape of stream operation: the bytes the file holds ahead of the body,
/// and the program that moves a body of `n` bytes. The program answers with
/// the number of bytes it moved.
pub struct Shape {
    pub name: &'static str,
    pub prefix: &'static [u8],
    pub program: fn(input: &Path, output: &Path, n: usize) -> String,
}

/// Rust heap bytes allocated per byte of body, between `SMALL` and `LARGE`.
pub fn slope(shape: &Shape) -> f64 {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let scratch = Scratch::new();
    let run = |n: usize| -> u64 {
        let input = scratch.0.join(format!("in-{n}"));
        let output = scratch.0.join(format!("out-{n}"));
        let mut content = shape.prefix.to_vec();
        content.extend(std::iter::repeat_n(b'x', n));
        std::fs::write(&input, &content).expect("write the input file");
        let source = (shape.program)(&input, &output, n);
        let before = ALLOCATED.load(Ordering::Relaxed);
        crate::common::eval_source(&source, |r| {
            let moved = r.unwrap_or_else(|e| panic!("{}: the program failed: {e}", shape.name));
            assert_eq!(
                moved.as_int(),
                Some(n as i64),
                "{}: the program must move every byte of the body",
                shape.name
            );
        });
        let spent = ALLOCATED.load(Ordering::Relaxed) - before;
        std::fs::remove_file(&input).ok();
        std::fs::remove_file(&output).ok();
        spent
    };
    // A first run pays for what the process initializes once.
    run(SMALL);
    let small = run(SMALL);
    let large = run(LARGE);
    (large as f64 - small as f64) / (LARGE - SMALL) as f64
}

/// A path as an Elle string literal.
pub fn lit(path: &Path) -> String {
    format!("{:?}", path.display().to_string())
}

/// `port/read-exact` of the whole body from a file that holds nothing else.
pub const READ: Shape = Shape {
    name: "read-exact",
    prefix: b"",
    program: |input, _, n| {
        format!(
            "(let [p (port/open-bytes {} :read) b (port/read-exact p {n})] (port/close p) (length b))",
            lit(input)
        )
    },
};

/// The same read after a `port/read-line` took more than its line, so the port
/// holds a remainder the read-exact has to answer with first.
pub const READ_AFTER_LINE: Shape = Shape {
    name: "read-exact after read-line",
    prefix: b"hdr\n",
    program: |input, _, n| {
        format!(
            "(let [p (port/open-bytes {} :read) h (port/read-line p) b (port/read-exact p {n})] \
             (port/close p) (length b))",
            lit(input)
        )
    },
};

/// A read of the body followed by a write of it to another file. The read is
/// pinned on its own above, so what this adds is the write.
pub const WRITE: Shape = Shape {
    name: "write",
    prefix: b"",
    program: |input, output, n| {
        format!(
            "(let [p (port/open-bytes {} :read) b (port/read-exact p {n}) \
                   q (port/open-bytes {} :write) w (port/write q b)] \
             (port/close p) (port/close q) w)",
            lit(input),
            lit(output)
        )
    },
};

/// `port/read-all` of the whole file, which copies once by design.
pub const READ_ALL: Shape = Shape {
    name: "read-all",
    prefix: b"",
    program: |input, _, _| {
        format!(
            "(let [p (port/open-bytes {} :read) b (port/read-all p)] (port/close p) (length b))",
            lit(input)
        )
    },
};

/// Assert that `shape` allocates less than `limit` bytes per byte it moves.
pub fn assert_slope_below(shape: &Shape, limit: f64, backend: &str) {
    let per_byte = slope(shape);
    eprintln!(
        "{backend}: {} allocates {per_byte:.2} bytes per byte",
        shape.name
    );
    assert!(
        per_byte < limit,
        "{backend}: {} allocated {per_byte:.2} Rust heap bytes per byte of body; \
         the limit is {limit} (docs/impl/io-bytes.md)",
        shape.name
    );
}
