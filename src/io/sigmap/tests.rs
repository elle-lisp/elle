//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn round_trip_every_keyword() {
    for (name, signum) in SIGNALS {
        assert_eq!(keyword_to_signum(name), Some(*signum));
        assert_eq!(signum_to_keyword(*signum), Some(*name));
    }
}

#[test]
fn unknown_keyword_is_none() {
    assert_eq!(keyword_to_signum("sigfoo"), None);
    assert_eq!(keyword_to_signum("SIGTERM"), None); // case-sensitive
    assert_eq!(keyword_to_signum(""), None);
}

#[test]
fn unknown_signum_is_none() {
    // 99 is not a standard signal on any platform we target.
    assert_eq!(signum_to_keyword(99), None);
    assert_eq!(signum_to_keyword(0), None);
    assert_eq!(signum_to_keyword(-1), None);
}

#[test]
fn supported_list_includes_common_signals() {
    let s = supported_list_str();
    assert!(s.contains(":sigterm"));
    assert!(s.contains(":sigkill"));
    assert!(s.contains(":sigint"));
    assert!(s.contains(":sigusr1"));
}
