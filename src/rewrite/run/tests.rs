use super::*;

#[test]
fn test_rewrite_preserves_shebang() {
    let source = "#!/usr/bin/env elle\n(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some(), "expected rewrites to be applied");
    let (new_source, _count) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    let expected_prefix = format!("#!/usr/bin/env elle\n{}", epoch_line);
    assert!(
        new_source.starts_with(&expected_prefix),
        "shebang then epoch tag expected, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
    // Old epoch tag must not survive
    assert!(
        !new_source.contains("(elle/epoch 0)"),
        "old epoch tag should be removed"
    );
    let epoch_count = new_source.matches("elle/epoch").count();
    assert_eq!(
        epoch_count, 1,
        "should have exactly one epoch tag, got: {:?}",
        new_source
    );
}

#[test]
fn test_rewrite_injects_epoch_first_form() {
    let source = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some());
    let (new_source, _) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    assert!(
        new_source.starts_with(&epoch_line),
        "epoch tag should be the first form, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
    assert!(
        !new_source.contains("(elle/epoch 0)"),
        "old epoch tag should be removed"
    );
    // Verify no double epoch tags
    let epoch_count = new_source.matches("elle/epoch").count();
    assert_eq!(
        epoch_count, 1,
        "should have exactly one epoch tag, got: {:?}",
        new_source
    );
}

#[test]
fn test_rewrite_no_epoch_tag_injects_one() {
    // File without an epoch tag gets one added (current epoch).
    let source = "(println \"hello\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some(), "epoch tag should be injected");
    let (new_source, _) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    assert!(
        new_source.starts_with(&epoch_line),
        "epoch tag should be first form, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
}
