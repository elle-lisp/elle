//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_empty_range() {
    let renames = collapsed_renames(0, 0);
    assert!(renames.is_empty());
}

#[test]
fn test_renames_through_current() {
    let renames = collapsed_renames(0, CURRENT_EPOCH);
    // epoch 2: print→println, newline→println
    // epoch 3: display→print
    // epoch 4: stream/{read,read-line,read-all,write,flush} → port/...
    assert_eq!(renames.get("print"), Some(&"println"));
    assert_eq!(renames.get("newline"), Some(&"println"));
    assert_eq!(renames.get("display"), Some(&"print"));
    assert_eq!(renames.get("stream/read-line"), Some(&"port/read-line"));
    assert_eq!(renames.get("stream/read"), Some(&"port/read"));
    assert_eq!(renames.get("stream/read-all"), Some(&"port/read-all"));
    assert_eq!(renames.get("stream/write"), Some(&"port/write"));
    assert_eq!(renames.get("stream/flush"), Some(&"port/flush"));
    // epoch 5: string-contains?→has?, string/contains?→has?
    assert_eq!(renames.get("string-contains?"), Some(&"has?"));
    assert_eq!(renames.get("string/contains?"), Some(&"has?"));
    // epoch 10: cons→pair, car→first, cdr→rest
    assert_eq!(renames.get("cons"), Some(&"pair"));
    assert_eq!(renames.get("car"), Some(&"first"));
    assert_eq!(renames.get("cdr"), Some(&"rest"));
    // epoch 11: sys/spawn→sys/spawn-vm, os/spawn→os/spawn-vm
    // (bare `spawn` is intentionally NOT renamed — it is also a local)
    assert_eq!(renames.get("spawn"), None);
    assert_eq!(renames.get("sys/spawn"), Some(&"sys/spawn-vm"));
    assert_eq!(renames.get("os/spawn"), Some(&"os/spawn-vm"));
    // epoch 12: coro/* → fiber/*, coroutine-* → fiber/*, etc.
    assert_eq!(renames.get("coro/resume"), Some(&"fiber/resume"));
    assert_eq!(renames.get("coro/status"), Some(&"fiber/status"));
    assert_eq!(renames.get("coro/done?"), Some(&"fiber/done?"));
    assert_eq!(renames.get("coro/value"), Some(&"fiber/value"));
    assert_eq!(renames.get("coroutine-resume"), Some(&"fiber/resume"));
    assert_eq!(renames.get("coroutine-status"), Some(&"fiber/status"));
    assert_eq!(renames.get("coroutine-done?"), Some(&"fiber/done?"));
    assert_eq!(renames.get("coroutine-value"), Some(&"fiber/value"));
    assert_eq!(renames.get("coroutine?"), Some(&"fiber?"));
    assert_eq!(renames.get("coro?"), Some(&"fiber?"));
    assert_eq!(renames.get("yield-from"), Some(&"yield*"));
    // 13 base renames (epochs 1–10) + 2 spawn (epoch 11) + 11 coro (epoch 12)
    assert_eq!(renames.len(), 26);
}

#[test]
fn test_replace_rules_empty_range() {
    let replaces = replace_rules_in_range(0, 0);
    assert!(replaces.is_empty());
}

#[test]
fn test_replace_rules_epoch_1() {
    let replaces = replace_rules_in_range(0, 1);
    assert_eq!(replaces.len(), 9);
    // First rule should be assert-true
    assert_eq!(replaces[0].0, "assert-true");
}

#[test]
fn test_removals_epoch_2() {
    let removals = removals_in_range(0, CURRENT_EPOCH);
    assert!(removals.contains_key("write"));
    // epoch 12: coro/>iterator, coroutine->iterator, coroutine-next
    assert!(removals.contains_key("coro/>iterator"));
    assert!(removals.contains_key("coroutine->iterator"));
    assert!(removals.contains_key("coroutine-next"));
    assert_eq!(removals.len(), 4);
}

#[test]
fn test_flatten_rules_epoch_7() {
    let flattens = flatten_rules_in_range(0, CURRENT_EPOCH);
    assert!(flattens.contains(&"let"));
    assert!(flattens.contains(&"letrec"));
    assert!(flattens.contains(&"let*"));
    assert!(flattens.contains(&"if-let"));
    assert!(flattens.contains(&"when-let"));
    assert!(flattens.contains(&"when-ok"));
    assert_eq!(flattens.len(), 6);
}

#[test]
fn test_rename_chaining() {
    // Simulate chained renames manually
    let mut table: HashMap<&str, &str> = HashMap::new();

    // Epoch 1: A → B
    table.insert("A", "B");

    // Epoch 2: B → C — should update A → C
    let original = table.iter().find(|(_, v)| **v == "B").map(|(k, _)| *k);
    if let Some(original) = original {
        table.insert(original, "C");
    } else {
        table.insert("B", "C");
    }

    assert_eq!(table.get("A"), Some(&"C"));
    assert!(!table.contains_key("B"));
}
