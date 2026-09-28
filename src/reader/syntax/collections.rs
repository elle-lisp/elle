// audited: 2026-09-28
//! Parses collection forms and reports unfinished delimiters.
//! docs/impl/reader.md

use super::super::token::{OwnedToken, SourceLoc};
use super::{OpenFormKind, SyntaxReader};
use crate::syntax::{Syntax, SyntaxKind};

impl SyntaxReader {
    pub(super) fn read_set(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Set, start_loc);
        self.advance(); // skip opening |
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::Pipe) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Set(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_set_mut(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Set, start_loc);
        self.advance(); // skip opening @|
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::Pipe) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::SetMut(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_bytes(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Bytes, start_loc);
        self.advance(); // skip b[
        let mut elements = Vec::new();
        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Bytes(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_bytes_mut(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Bytes, start_loc);
        self.advance(); // skip @b[
        let mut elements = Vec::new();
        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::BytesMut(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_list(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::List, start_loc);
        self.advance(); // skip (
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::RightParen) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::List(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_array(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Array, start_loc);
        self.advance(); // skip [
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::RightBracket) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Array(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_struct(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.open_form(OpenFormKind::Struct, start_loc);
        self.advance(); // skip {
        let mut elements = Vec::new();

        loop {
            match self.current() {
                None => return Err(self.unterminated_collection()),
                Some(OwnedToken::RightBrace) => {
                    let end = self.current_byte_offset() + self.current_length();
                    self.advance();
                    self.close_form();
                    let span = self.make_span(start_boff, end, start_loc);
                    return Ok(Syntax::new(
                        SyntaxKind::Struct(self.arena.nodes(&elements)),
                        span,
                    ));
                }
                Some(OwnedToken::Comment(_)) => {
                    self.advance();
                    continue;
                }
                Some(OwnedToken::Pipe) => {
                    let set_loc = self.current_location();
                    let set_boff = self.current_byte_offset();
                    elements.push(self.read_set(&set_loc, set_boff)?);
                    continue;
                }
                _ => elements.push(self.read()?),
            }
        }
    }

    pub(super) fn read_list_sugar(
        &mut self,
        start_loc: &SourceLoc,
        start_boff: usize,
    ) -> Result<Syntax, String> {
        self.advance(); // skip @

        match self.current() {
            Some(OwnedToken::LeftBracket) => {
                // @[...] produces an array literal
                self.open_form(OpenFormKind::Array, start_loc);
                self.advance(); // skip [
                let mut elements = Vec::new();

                loop {
                    match self.current() {
                        None => return Err(self.unterminated_collection()),
                        Some(OwnedToken::RightBracket) => {
                            let end = self.current_byte_offset() + self.current_length();
                            self.advance();
                            self.close_form();
                            let span = self.make_span(start_boff, end, start_loc);
                            return Ok(Syntax::new(
                                SyntaxKind::ArrayMut(self.arena.nodes(&elements)),
                                span,
                            ));
                        }
                        Some(OwnedToken::Comment(_)) => {
                            self.advance();
                            continue;
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::LeftBrace) => {
                // @{...} produces a table literal
                self.open_form(OpenFormKind::Struct, start_loc);
                self.advance(); // skip {
                let mut elements = Vec::new();

                loop {
                    match self.current() {
                        None => return Err(self.unterminated_collection()),
                        Some(OwnedToken::RightBrace) => {
                            let end = self.current_byte_offset() + self.current_length();
                            self.advance();
                            self.close_form();
                            let span = self.make_span(start_boff, end, start_loc);
                            return Ok(Syntax::new(
                                SyntaxKind::StructMut(self.arena.nodes(&elements)),
                                span,
                            ));
                        }
                        Some(OwnedToken::Comment(_)) => {
                            self.advance();
                            continue;
                        }
                        _ => elements.push(self.read()?),
                    }
                }
            }
            Some(OwnedToken::String(s)) => {
                // @"..." is a mutable string literal
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
