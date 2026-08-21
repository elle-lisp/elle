// LSP 3.17 spec-compliance tests for the Elle language server.
//
// This file is the include! coordinator: it owns the shared `use` and
// wires the themed subfiles. Each subfile starts with `use super::*;`
// so the shared imports resolve here. Tests are grouped by the spec
// section banners they originally carried.
use serde_json::json;

mod messages {
    include!("compliance/messages.rs");
}
mod types {
    include!("compliance/types.rs");
}
