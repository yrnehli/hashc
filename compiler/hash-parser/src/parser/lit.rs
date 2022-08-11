//! Hash Compiler AST generation sources. This file contains the sources to the
//! logic that transforms tokens into an AST.
use hash_ast::ast::*;
use hash_source::{
    constant::{IntConstant, CONSTANT_MAP},
    identifier::{Identifier, CORE_IDENTIFIERS},
    location::Span,
};
use hash_token::{delimiter::Delimiter, keyword::Keyword, Token, TokenKind, TokenKindVector};

use super::AstGen;
use crate::diagnostics::error::{NumericLitKind, ParseErrorKind, ParseResult};

impl<'stream, 'resolver> AstGen<'stream, 'resolver> {
    /// Parse a primitive literal, which means it can be either a `char`,
    /// `integer`, `float` or a `string`.
    pub(crate) fn parse_primitive_lit(&self) -> ParseResult<AstNode<Lit>> {
        let token = self.current_token();

        Ok(self.node_with_span(
            match token.kind {
                TokenKind::IntLit(value) => {
                    let IntConstant { suffix, .. } = CONSTANT_MAP.lookup_int_constant(value);

                    // Parse the provided suffix
                    let kind = suffix.map_or(Ok(IntLitKind::Unsuffixed), |s| {
                        self.parse_integer_suffix(s, token.span)
                    })?;

                    Lit::Int(IntLit { value, kind })
                }
                TokenKind::FloatLit(value) => {
                    let suffix = CONSTANT_MAP.lookup_float_constant(value).suffix;

                    // Parse the provided suffix
                    let kind = suffix.map_or(Ok(FloatLitKind::Unsuffixed), |s| {
                        self.parse_float_suffix(s, token.span)
                    })?;

                    Lit::Float(FloatLit { value, kind })
                }
                TokenKind::CharLit(value) => Lit::Char(CharLit(value)),
                TokenKind::StrLit(value) => Lit::Str(StrLit(value)),
                TokenKind::Keyword(Keyword::False) => Lit::Bool(BoolLit(false)),
                TokenKind::Keyword(Keyword::True) => Lit::Bool(BoolLit(true)),
                _ => self.err_with_location(
                    ParseErrorKind::ExpectedLit,
                    None,
                    Some(token.kind),
                    token.span,
                )?,
            },
            token.span,
        ))
    }

    /// Parse an integer literal suffix.
    fn parse_integer_suffix(&self, suffix: Identifier, span: Span) -> ParseResult<IntLitKind> {
        let ty = match suffix {
            id if CORE_IDENTIFIERS.i8 == id => IntTy::I8,
            id if CORE_IDENTIFIERS.i16 == id => IntTy::I16,
            id if CORE_IDENTIFIERS.i32 == id => IntTy::I32,
            id if CORE_IDENTIFIERS.i64 == id => IntTy::I64,
            id if CORE_IDENTIFIERS.i128 == id => IntTy::I128,
            id if CORE_IDENTIFIERS.isize == id => IntTy::ISize,
            id if CORE_IDENTIFIERS.ibig == id => IntTy::IBig,
            id if CORE_IDENTIFIERS.u8 == id => IntTy::U8,
            id if CORE_IDENTIFIERS.u16 == id => IntTy::U16,
            id if CORE_IDENTIFIERS.u32 == id => IntTy::U32,
            id if CORE_IDENTIFIERS.u64 == id => IntTy::U64,
            id if CORE_IDENTIFIERS.u128 == id => IntTy::U128,
            id if CORE_IDENTIFIERS.usize == id => IntTy::USize,
            id if CORE_IDENTIFIERS.ubig == id => IntTy::UBig,
            id => self.err_with_location(
                ParseErrorKind::InvalidLitSuffix(NumericLitKind::Integer, id),
                None,
                None,
                span,
            )?,
        };

        Ok(IntLitKind::Suffixed(ty))
    }

    /// Parse an integer literal suffix.
    fn parse_float_suffix(&self, suffix: Identifier, span: Span) -> ParseResult<FloatLitKind> {
        match suffix {
            id if CORE_IDENTIFIERS.f32 == id => Ok(FloatLitKind::Suffixed(FloatTy::F32)),
            id if CORE_IDENTIFIERS.f64 == id => Ok(FloatLitKind::Suffixed(FloatTy::F64)),
            id => self.err_with_location(
                ParseErrorKind::InvalidLitSuffix(NumericLitKind::Float, id),
                None,
                None,
                span,
            )?,
        }
    }

    /// Parse a single map entry in a literal.
    pub(crate) fn parse_map_entry(&mut self) -> ParseResult<AstNode<MapLitEntry>> {
        let start = self.current_location();

        let key = self.parse_expr_with_precedence(0)?;
        self.parse_token(TokenKind::Colon)?;
        let value = self.parse_expr_with_precedence(0)?;

        Ok(self.node_with_joined_span(MapLitEntry { key, value }, start))
    }

    /// Parse a map literal which is made of braces with an arbitrary number of
    /// fields separated by commas.
    pub(crate) fn parse_map_lit(&mut self) -> ParseResult<AstNode<Lit>> {
        debug_assert!(self.current_token().has_kind(TokenKind::Keyword(Keyword::Map)));

        let start = self.current_location();
        let mut gen = self.parse_delim_tree(Delimiter::Brace, None)?;

        let elements =
            gen.parse_separated_fn(|g| g.parse_map_entry(), |g| g.parse_token(TokenKind::Comma));
        self.consume_gen(gen);

        Ok(self.node_with_joined_span(Lit::Map(MapLit { elements }), start))
    }

    /// Parse a set literal which is made of braces with an arbitrary number of
    /// fields separated by commas.
    pub(crate) fn parse_set_lit(&mut self) -> ParseResult<AstNode<Lit>> {
        debug_assert!(self.current_token().has_kind(TokenKind::Keyword(Keyword::Set)));

        let start = self.current_location();
        let mut gen = self.parse_delim_tree(Delimiter::Brace, None)?;

        let elements = gen.parse_separated_fn(
            |g| g.parse_expr_with_precedence(0),
            |g| g.parse_token(TokenKind::Comma),
        );
        self.consume_gen(gen);

        Ok(self.node_with_joined_span(Lit::Set(SetLit { elements }), start))
    }

    /// Function to parse a tuple literal entry with a name.
    pub(crate) fn parse_tuple_lit_entry(&mut self) -> ParseResult<AstNode<TupleLitEntry>> {
        let start = self.next_location();
        let offset = self.offset();

        // Determine if this might have a tuple field name and optional type
        let entry = if let Some(name) = self.peek_resultant_fn(|g| g.parse_name()) {
            // Here we can identify if we need to backtrack and just parse an expression...
            if !matches!(self.peek(), Some(Token { kind: TokenKind::Colon | TokenKind::Eq, .. })) {
                self.offset.set(offset);
                None
            } else {
                // Try and parse an optional type...
                let ty = match self.peek() {
                    Some(token) if token.has_kind(TokenKind::Colon) => {
                        self.skip_token();

                        match self.peek() {
                            Some(token) if token.has_kind(TokenKind::Eq) => None,
                            _ => Some(self.parse_type()?),
                        }
                    }
                    _ => None,
                };

                self.parse_token_fast(TokenKind::Eq).ok_or_else(|| {
                    self.make_err(
                        ParseErrorKind::ExpectedValueAfterTyAnnotation,
                        Some(TokenKindVector::singleton(TokenKind::Eq)),
                        None,
                        Some(self.next_location()),
                    )
                })?;

                let value = self.parse_expr_with_re_assignment()?.0;

                // Now we try and parse an expression that allows re-assignment operators...
                Some(
                    self.node_with_joined_span(
                        TupleLitEntry { name: Some(name), ty, value },
                        start,
                    ),
                )
            }
        } else {
            None
        };

        match entry {
            Some(entry) => Ok(entry),
            None => {
                let value = self.parse_expr_with_re_assignment()?.0;

                Ok(self.node_with_joined_span(TupleLitEntry { name: None, ty: None, value }, start))
            }
        }
    }

    /// Parse an list literal from a given token tree.
    pub(crate) fn parse_list_lit(
        &self,
        tree: &'stream [Token],
        span: Span,
    ) -> ParseResult<AstNode<Expr>> {
        let mut gen = self.from_stream(tree, span);
        let mut elements = AstNodes::empty();

        while gen.has_token() {
            let expr = gen.parse_expr_with_precedence(0)?;
            elements.nodes.push(expr);

            match gen.peek() {
                Some(token) if token.has_kind(TokenKind::Comma) => {
                    gen.skip_token();
                }
                Some(token) => {
                    // if we haven't exhausted the whole token stream, then report this as a
                    // unexpected token error
                    return gen.err(
                        ParseErrorKind::Expected,
                        Some(TokenKindVector::singleton(TokenKind::Comma)),
                        Some(token.kind),
                    );
                }
                None => break,
            }
        }

        Ok(gen.node_with_span(
            Expr::new(ExprKind::LitExpr(LitExpr(
                gen.node_with_span(Lit::List(ListLit { elements }), span),
            ))),
            span,
        ))
    }
}
