//! Contains structures related to literals, like numbers, strings, etc.
use std::fmt::Display;

use hash_ast::ast;
use hash_source::constant::{InternedFloat, InternedInt, InternedStr};
use hash_target::size::Size;
use num_bigint::BigInt;

/// An integer literal.
///
/// Uses the `ast` representation.
#[derive(Copy, Clone, Debug)]
pub struct IntLit {
    pub underlying: ast::IntLit,
}

impl IntLit {
    /// Get the interned value of the literal.
    pub fn interned_value(&self) -> InternedInt {
        self.underlying.value
    }

    /// Return the value of the integer literal.
    pub fn value(&self) -> BigInt {
        (&self.underlying.value.value()).try_into().unwrap()
    }
}

impl From<InternedInt> for IntLit {
    fn from(value: InternedInt) -> Self {
        Self { underlying: ast::IntLit { value, kind: ast::IntLitKind::Unsuffixed } }
    }
}

/// A string literal.
///
/// Uses the `ast` representation.
#[derive(Copy, Clone, Debug)]
pub struct StrLit {
    pub underlying: ast::StrLit,
}

impl StrLit {
    /// Get the interned value of the literal.
    pub fn interned_value(&self) -> InternedStr {
        self.underlying.data
    }

    /// Return the value of the string literal.
    pub fn value(&self) -> &'static str {
        self.underlying.data.value()
    }
}

impl From<InternedStr> for StrLit {
    fn from(value: InternedStr) -> Self {
        Self { underlying: ast::StrLit { data: value } }
    }
}

/// A float literal.
///
/// Uses the `ast` representation.
#[derive(Copy, Clone, Debug)]
pub struct FloatLit {
    pub underlying: ast::FloatLit,
}

impl FloatLit {
    /// Get the interned value of the literal.
    pub fn interned_value(&self) -> InternedFloat {
        self.underlying.value
    }

    /// Return the value of the float literal.
    pub fn value(&self) -> f64 {
        self.underlying.value.value().as_f64()
    }
}

impl From<InternedFloat> for FloatLit {
    fn from(value: InternedFloat) -> Self {
        Self { underlying: ast::FloatLit { value, kind: ast::FloatLitKind::Unsuffixed } }
    }
}

/// A character literal.
///
/// Uses the `ast` representation.
#[derive(Copy, Clone, Debug)]
pub struct CharLit {
    pub underlying: ast::CharLit,
}

impl CharLit {
    /// Return the value of the character literal.
    pub fn value(&self) -> char {
        self.underlying.data
    }
}

impl From<char> for CharLit {
    fn from(data: char) -> Self {
        Self { underlying: ast::CharLit { data } }
    }
}

/// A literal
#[derive(Copy, Clone, Debug)]
pub enum Lit {
    Int(IntLit),
    Str(StrLit),
    Char(CharLit),
    Float(FloatLit),
}

/// A literal pattern
///
/// This is a literal that can appear in a pattern, which does not include
/// floats.
#[derive(Copy, Clone, Debug)]
pub enum LitPat {
    Int(IntLit),
    Str(StrLit),
    Char(CharLit),
}

impl From<LitPat> for Lit {
    fn from(val: LitPat) -> Self {
        match val {
            LitPat::Int(l) => Lit::Int(l),
            LitPat::Str(l) => Lit::Str(l),
            LitPat::Char(l) => Lit::Char(l),
        }
    }
}

impl Display for IntLit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.underlying.value)
    }
}

impl Display for StrLit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.underlying.data)
    }
}

impl Display for CharLit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.underlying.data)
    }
}

impl Display for FloatLit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.underlying.value)
    }
}

impl Display for LitPat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            // It's often the case that users don't include the range of the entire
            // integer and so we will write `-2147483648..x` and
            // same for max, what we want to do is write `MIN`
            // and `MAX` for these situations since it is easier for the
            // user to understand the problem.
            LitPat::Int(lit) => {
                let kind = lit.interned_value().map(|constant| constant.ty());

                // @@Hack: we don't use size since it is never invoked because of
                // integer constant don't store usize values.
                let dummy_size = Size::ZERO;

                if !kind.is_bigint() {
                    let value =
                        lit.interned_value().map(|constant| constant.value.as_u128().unwrap());

                    if kind.numeric_min(dummy_size) == value {
                        write!(f, "{kind}::MIN")
                    } else if kind.numeric_max(dummy_size) == value {
                        write!(f, "{kind}::MAX")
                    } else {
                        write!(f, "{lit}")
                    }
                } else {
                    write!(f, "{lit}")
                }
            }
            LitPat::Str(lit) => write!(f, "{lit}"),
            LitPat::Char(lit) => write!(f, "{lit}"),
        }
    }
}

impl Display for Lit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Lit::Int(lit) => write!(f, "{lit}"),
            Lit::Str(lit) => write!(f, "{lit}"),
            Lit::Char(lit) => write!(f, "{lit}"),
            Lit::Float(lit) => write!(f, "{lit}"),
        }
    }
}
