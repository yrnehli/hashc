//! Hash Compiler language keyword definitions
//!
//! All rights reserved 2021 (c) The Hash Language authors

use std::fmt;
use strum::IntoEnumIterator;
use strum_macros::{AsRefStr, EnumIter, EnumString, EnumVariantNames};

#[derive(Debug, Copy, Clone, PartialEq, AsRefStr, EnumString, EnumIter, EnumVariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum Keyword {
    Let,
    For,
    While,
    Loop,
    If,
    Else,
    Match,
    As,
    In,
    Where,
    Trait,
    Enum,
    Struct,
    Continue,
    Break,
    Return,
}

pub fn get_variants() -> Vec<Keyword> {
    Keyword::iter().collect()
}

/// Enum Variants for keywords
// const keywords: [Keyword; Keyword::VARIANTS.len()] = Keyword::iter().collect::<[Keyword; Keyword::VARIANTS.len()]>();

impl fmt::Display for Keyword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}
