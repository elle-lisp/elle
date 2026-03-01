use crate::pipeline::eval;
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// Define fn/dot-escape, fn/graph, and fn/save-graph as Elle functions.
pub fn define_graph_functions(vm: &mut VM, symbols: &mut SymbolTable) {
    let dot_escape_code = r#"
        (defn fn/dot-escape (s)
          "Escape special DOT record-label characters."
          (-> s
            (string/replace "{" "\\{")
            (string/replace "}" "\\}")
            (string/replace "|" "\\|")
            (string/replace "<" "\\<")
            (string/replace ">" "\\>")))
    "#;

    let graph_code = r#"
        (defn fn/graph (cfg)
          "Convert a fn/flow CFG struct to DOT format string."
          (let* ((name (get cfg :name))
                 (doc (get cfg :doc))
                 (label (if (nil? name)
                          (if (nil? doc) "anonymous" doc)
                          name))
                 (result (-> "digraph {\n  label=\""
                           (append label)
                           (append " arity:")
                           (append (get cfg :arity))
                           (append " regs:")
                           (append (string (get cfg :regs)))
                           (append " locals:")
                           (append (string (get cfg :locals)))
                           (append "\";\n  node [shape=record];\n"))))
            (each block (get cfg :blocks)
              (let* ((lbl (string (get block :label)))
                     (instrs (get block :instrs))
                     (term (get block :term))
                     (edges (get block :edges)))
                (set result (-> result
                              (append "  block")
                              (append lbl)
                              (append " [label=\"{block")
                              (append lbl)))
                (set result (append result "|"))
                (each instr instrs
                  (set result (-> result
                                (append (fn/dot-escape instr))
                                (append "\\l"))))
                (set result (-> result
                              (append "|")
                              (append (fn/dot-escape term))
                              (append "}\"];\n")))
                (each edge edges
                  (set result (-> result
                                (append "  block")
                                (append lbl)
                                (append " -> block")
                                (append (string edge))
                                (append ";\n"))))))
            (append result "}\n")))
    "#;

    let save_graph_code = r#"
        (defn fn/save-graph (closure path)
          "Save the LIR control flow graph of a closure as a DOT file."
          (file/write path (fn/graph (fn/flow closure))))
    "#;

    for (name, code) in &[
        ("fn/dot-escape", dot_escape_code),
        ("fn/graph", graph_code),
        ("fn/save-graph", save_graph_code),
    ] {
        if let Err(e) = eval(code, symbols, vm) {
            eprintln!("Warning: Failed to define {}: {}", name, e);
        }
    }
}
