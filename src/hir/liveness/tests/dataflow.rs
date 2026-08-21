use super::*;

#[test]
fn test_dead_binding() {
    let (arena, symbols, info) = analyze("(let [x 1] 42)");
    if let Some(x) = find_binding(&info, &arena, &symbols, "x") {
        assert!(
            !is_live_anywhere(&info, x),
            "dead binding x should not be live"
        );
    }
}

#[test]
fn test_live_binding() {
    let (arena, symbols, info) = analyze("(let [x 1] x)");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert!(
        is_live_anywhere(&info, x),
        "x should be live between def and use"
    );
}

#[test]
fn test_if_branch_liveness() {
    let (arena, symbols, info) = analyze("(let [x 1] (if (cond_var) x 2))");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert!(is_live_anywhere(&info, x), "x should be live before if");
}

#[test]
fn test_loop_liveness() {
    let (arena, symbols, info) = analyze("(begin (def @i 0) (while (< i 10) (set i (+ i 1))))");
    let i_bindings: Vec<Binding> = info
        .def_site
        .keys()
        .filter(|&&b| symbols.name(arena.get(b).name) == Some("i"))
        .copied()
        .collect();
    assert!(!i_bindings.is_empty());
    assert!(
        i_bindings.iter().any(|&b| is_live_anywhere(&info, b)),
        "loop param i should be live across iterations"
    );
}

#[test]
fn test_lambda_capture_liveness() {
    let (arena, symbols, info) = analyze("(let [x 1] (let [ff (fn () x)] (ff)))");
    let x = find_binding(&info, &arena, &symbols, "x").expect("x not found");
    assert!(
        is_live_anywhere(&info, x),
        "captured x should be live at lambda"
    );
}

// ── per-HirId last-use tests ─────────────────────────────────────
