//! Language Server Protocol implementation for Elle Lisp

pub mod completion;
pub mod definition;
pub mod documentsymbol;
pub mod formatting;
pub mod hover;
pub mod locate;
pub mod references;
pub mod rename;
pub mod run;
pub mod state;

pub use state::CompilerState;

#[cfg(test)]
pub(crate) mod testutil {
    //! Shared test scaffolding: compile a source string into a resident
    //! `CompilerState` so provider tests can run against a *real* symbol index
    //! built from source, not a hand-rolled empty one.
    use super::CompilerState;

    /// Open `src` under `uri` and compile it. Call inside `with_test_region`.
    pub(crate) fn compiled(uri: &str, src: &str) -> CompilerState {
        let mut state = CompilerState::new();
        state.on_document_open(uri.to_string(), src.to_string());
        state.compile_document(uri);
        state
    }
}
