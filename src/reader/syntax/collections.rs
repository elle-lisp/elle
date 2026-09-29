// audited: 2026-09-28
//! Parses collection forms and reports unfinished delimiters.
//! docs/impl/reader.md

use super::super::token::{OwnedToken, SourceLoc};
use super::{OpenFormKind, SyntaxReader};
use crate::syntax::{SeqCtor, Syntax, SyntaxKind};

impl SyntaxReader {
    /// Read one collection: skip its opening token, read elements until
    /// `close`, and build the node with `make`.
    ///
    /// The collection is on `open_forms` while its elements are read, so an
    /// unterminated-input error names the outermost collection still open and
    /// counts all of them. Every exit truncates `open_forms` to the depth it
    /// had on entry, so an error inside the collection leaves nothing behind
    /// for the next read to blame.
    fn read_delimited(
        &mut self,
        kind: OpenFormKind,
        close: OwnedToken,
        make: SeqCtor,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        let depth = self.open_forms.len();
        self.open_form(kind, start_loc);
        self.advance();
        let result = self.read_elements(close, make, start_loc, start_boff);
        self.open_forms.truncate(depth);
        result
    }

    fn read_elements(
        &mut self,
        close: OwnedToken,
        make: SeqCtor,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        let mut elements = Vec::new();
        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(token) if *token == close => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(make(self.arena.nodes(&elements)), span));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_set(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Set,
            OwnedToken::Pipe,
            SyntaxKind::Set,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_set_mut(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Set,
            OwnedToken::Pipe,
            SyntaxKind::SetMut,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_bytes(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Bytes,
            OwnedToken::RightBracket,
            SyntaxKind::Bytes,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_bytes_mut(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Bytes,
            OwnedToken::RightBracket,
            SyntaxKind::BytesMut,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_list(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::List,
            OwnedToken::RightParen,
            SyntaxKind::List,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_array(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Array,
            OwnedToken::RightBracket,
            SyntaxKind::Array,
            start_loc,
            start_boff,
        )
    }

    pub(super) fn read_struct(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.read_delimited(
            OpenFormKind::Struct,
            OwnedToken::RightBrace,
            SyntaxKind::Struct,
            start_loc,
            start_boff,
        )
    }

    /// `@` before `[`, `{` or a string: the mutable form of that literal. The
    /// node's span starts at the `@`.
    pub(super) fn read_list_sugar(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.advance(); // skip @

        match self.current() {
            Some(OwnedToken::LeftBracket) => self.read_delimited(
                OpenFormKind::Array,
                OwnedToken::RightBracket,
                SyntaxKind::ArrayMut,
                start_loc,
                start_boff,
            ),
            Some(OwnedToken::LeftBrace) => self.read_delimited(
                OpenFormKind::Struct,
                OwnedToken::RightBrace,
                SyntaxKind::StructMut,
                start_loc,
                start_boff,
            ),
            Some(OwnedToken::String(s)) => {
                let string_val = self.arena.text(s);
                let end = self.current_byte_offset() + self.current_length();
                self.advance(); // skip the string token
                let span = self.make_span(start_boff, end, start_loc);
                Ok(Syntax::new(SyntaxKind::StringMut(string_val), span))
            }
            _ => Err(format!(
                "{}: @ must be followed by [...], {{...}}, |...|, or \"...\"",
                start_loc.position()
            )),
        }
    }
}
