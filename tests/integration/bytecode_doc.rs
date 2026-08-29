// The instruction set as documented must match the instruction set as defined.
//
// `docs/impl/bytecode.md` and `src/compiler/AGENTS.md` are prose about an enum.
// Nothing compiles them, so a name they spell is a claim no build checks. An
// instruction that was renamed, retired, or never existed reads as real to
// anyone who trusts the document, and the disagreement surfaces only when a
// reader goes looking for the opcode and cannot find it. The same holds for the
// file each document names as the enum's home: the enum can move without
// breaking a single reference, because the references are text.
//
// These tests read `Instruction` itself, and walk `src/` for the definition, so
// nothing here has to be edited when a variant is added or a module is split.

use elle::compiler::Instruction;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_doc(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|e| panic!("read {relative}: {e}"))
}

/// Every `Instruction` variant, by its Rust name.
///
/// `from_byte` rejects every byte past the last variant, so a walk over the
/// whole byte range yields exactly the variants that exist. The enum stays the
/// authority: adding, renaming, or retiring a variant changes this set with no
/// edit here.
fn instruction_names() -> BTreeSet<String> {
    (0..=u8::MAX)
        .filter_map(Instruction::from_byte)
        .map(|instr| format!("{instr:?}"))
        .collect()
}

/// The instruction names at the head of one documentation line.
///
/// The category blocks put a signature column first and prose second —
/// `MakeArrayMut n   construct @array from n stack values`, or a comma-joined
/// row like `Lt, Gt, Le, Ge   ordering comparisons`. An instruction name starts
/// with a capital and is alphanumeric; an operand name (`n`, `idx`, `offset`)
/// and every word of the prose do not. So collect from the left and stop at the
/// first token that is not name-shaped, which is where the description begins.
///
/// The trap: a prose column separated by a single space, as in
/// `JumpIfFalse offset branch if top is falsy`. Splitting the line on a run of
/// two or more spaces reads that whole line as the signature; stopping at the
/// first lowercase token reads it correctly.
fn leading_names(line: &str) -> Vec<String> {
    let mut names = Vec::new();
    for token in line.split_whitespace() {
        let token = token.trim_end_matches(',');
        let name_shaped = token.starts_with(|c: char| c.is_ascii_uppercase())
            && token.chars().all(|c| c.is_ascii_alphanumeric());
        if !name_shaped {
            break;
        }
        names.push(token.to_string());
    }
    names
}

/// Every instruction name the `## Instruction categories` section spells, each
/// with the line number it was found on.
fn documented_instructions(doc: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut in_section = false;
    let mut in_block = false;

    for (index, line) in doc.lines().enumerate() {
        if line.starts_with("## ") {
            in_section = line == "## Instruction categories";
            continue;
        }
        if line.starts_with("```") {
            in_block = in_section && !in_block;
            continue;
        }
        if in_block {
            found.extend(leading_names(line).into_iter().map(|n| (index + 1, n)));
        }
    }
    found
}

/// The paths the `## Files` block of `docs/impl/bytecode.md` names, each with
/// its line number. Every entry is repository-relative and sits first on its
/// line, ahead of the description column.
fn documented_files(doc: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut in_section = false;
    let mut in_block = false;

    for (index, line) in doc.lines().enumerate() {
        if line.starts_with("## ") {
            in_section = line == "## Files";
            continue;
        }
        if line.starts_with("```") {
            in_block = in_section && !in_block;
            continue;
        }
        if in_block {
            if let Some(path) = line.split_whitespace().next() {
                found.push((index + 1, path.to_string()));
            }
        }
    }
    found
}

/// Every `*.rs` path spelled anywhere on one line, with markdown decoration
/// (backticks, table pipes) stripped. A path is a run of the characters a path
/// can contain, so a name in a table cell reads the same as one in prose.
fn rust_paths(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || "._/-".contains(c)))
        .filter(|token| token.ends_with(".rs"))
        .map(str::to_string)
        .collect()
}

/// The single `.rs` file under `src/` that defines the instruction set. Found
/// by walking the tree rather than by naming it, so this test cannot be
/// satisfied by a document and a stale constant agreeing with each other.
fn instruction_enum_home() -> PathBuf {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
            let path = entry.expect("directory entry").path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                let text = fs::read_to_string(&path).unwrap_or_default();
                if text.contains("pub enum Instruction {") {
                    out.push(path);
                }
            }
        }
    }

    let mut homes = Vec::new();
    walk(&repo_root().join("src"), &mut homes);
    assert_eq!(
        homes.len(),
        1,
        "expected exactly one definition of `pub enum Instruction` under src/, found {homes:?}"
    );
    homes.pop().expect("one home")
}

// The category blocks are the reader's index of the instruction set, and an
// invented name there costs more than a missing one: it sends a reader looking
// for an opcode that was never emitted, decoded, or JIT-compiled. The
// counter-factual: `MakeArray`, `MakeStruct`, and `Yield` all read as ordinary
// entries for as long as nobody tried to grep them, and every gate ahead of the
// merge passed while they sat in the document.
#[test]
fn bytecode_doc_names_only_instructions_that_exist() {
    let doc = read_doc("docs/impl/bytecode.md");
    let documented = documented_instructions(&doc);

    // A parser that matches nothing satisfies the check below vacuously, so
    // pin both the volume and three names spanning the section's extremes.
    assert!(
        documented.len() > 20,
        "found only {} instruction names in the category blocks of \
         docs/impl/bytecode.md; the parser is broken, not the document",
        documented.len()
    );
    for anchor in ["LoadConst", "Emit", "DecrefRegion"] {
        assert!(
            documented.iter().any(|(_, name)| name == anchor),
            "the category blocks no longer name `{anchor}`; if the document was \
             restructured, teach this test the new shape — do not let it pass by \
             matching less"
        );
    }

    let real = instruction_names();
    let invented: Vec<String> = documented
        .iter()
        .filter(|(_, name)| !real.contains(name))
        .map(|(line, name)| format!("bytecode.md:{line}: {name}"))
        .collect();
    assert!(
        invented.is_empty(),
        "docs/impl/bytecode.md names instructions the `Instruction` enum does \
         not have: {invented:?}"
    );
}

// The `## Files` block is where a reader goes to find the code behind the
// prose. A path that no longer resolves sends them into an empty directory, and
// splitting a module is exactly the edit that leaves it stale.
#[test]
fn bytecode_doc_files_section_names_paths_that_exist() {
    let doc = read_doc("docs/impl/bytecode.md");
    let documented = documented_files(&doc);
    assert!(
        !documented.is_empty(),
        "docs/impl/bytecode.md § Files names no paths; the parser is broken, \
         not the document"
    );

    let root = repo_root();
    let missing: Vec<String> = documented
        .iter()
        .filter(|(_, path)| !root.join(path).exists())
        .map(|(line, path)| format!("bytecode.md:{line}: {path}"))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/impl/bytecode.md § Files names paths that do not exist: {missing:?}"
    );
}

// Both documents tell a reader which file holds the instruction set. The enum
// moved into its own module and neither reference moved with it — a rename
// nothing could catch, because a document that points at a file which still
// exists, and still holds bytecode machinery, looks right.
#[test]
fn the_instruction_enum_lives_where_the_docs_say() {
    let home = instruction_enum_home();
    let root = repo_root();

    for doc_path in ["docs/impl/bytecode.md", "src/compiler/AGENTS.md"] {
        let doc = read_doc(doc_path);
        let doc_dir = root.join(doc_path).parent().expect("doc has a parent").to_path_buf();
        let mut claims = 0;

        for (index, line) in doc.lines().enumerate() {
            // The lines that place the enum: they name it and call it an enum.
            // A line that mentions neither, or names no source file, makes no
            // claim about where it lives.
            if !(line.contains("Instruction") && line.contains("enum")) {
                continue;
            }
            let paths = rust_paths(line);
            if paths.is_empty() {
                continue;
            }
            claims += 1;
            // A document beside the code names its neighbours relatively; one
            // under docs/ spells the path from the repository root.
            let names_home = paths
                .iter()
                .any(|p| root.join(p) == home || doc_dir.join(p) == home);
            assert!(
                names_home,
                "{doc_path}:{}: places the `Instruction` enum in {paths:?}, but it \
                 is defined in {}",
                index + 1,
                home.strip_prefix(&root).unwrap_or(&home).display()
            );
        }

        assert!(
            claims > 0,
            "{doc_path} no longer says where the `Instruction` enum lives; if the \
             document was restructured, teach this test the new shape — do not let \
             it pass by matching nothing"
        );
    }
}
