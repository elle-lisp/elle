//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_alloc_returns_distinct_handles() {
    let mut pool = BufferPool::new();
    let h1 = pool.alloc(64);
    let h2 = pool.alloc(64);
    assert_ne!(h1, h2);
}

#[test]
fn test_release_and_reuse() {
    let mut pool = BufferPool::new();
    let h1 = pool.alloc(64);
    pool.release(h1);
    let h2 = pool.alloc(128);
    // Reuses the same slot
    assert_eq!(h1, h2);
    // But the buffer has the new size
    assert_eq!(pool.get_mut(h2).len(), 128);
}

#[test]
fn test_get_mut_returns_correct_buffer() {
    let mut pool = BufferPool::new();
    let h = pool.alloc(4);
    let buf = pool.get_mut(h);
    buf[0] = 0xAA;
    buf[1] = 0xBB;
    assert_eq!(pool.get_mut(h)[0], 0xAA);
    assert_eq!(pool.get_mut(h)[1], 0xBB);
}

#[test]
fn test_alloc_zeroed() {
    let mut pool = BufferPool::new();
    let h = pool.alloc(16);
    let buf = pool.get_mut(h);
    assert!(buf.iter().all(|&b| b == 0));
}

#[test]
fn test_release_returns_contents() {
    let mut pool = BufferPool::new();
    let h = pool.alloc(4);
    pool.get_mut(h)[0] = 42;
    let returned = pool.release(h);
    assert_eq!(returned[0], 42);
}

#[test]
#[should_panic(expected = "double release")]
fn test_double_release_panics() {
    let mut pool = BufferPool::new();
    let h = pool.alloc(4);
    pool.release(h);
    pool.release(h);
}
