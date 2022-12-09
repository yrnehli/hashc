//! General definition-related utilities

use std::fmt::Display;

use hash_utils::{
    new_sequence_store_key,
    store::{CloneStore, DefaultSequenceStore, SequenceStore},
};
use utility_types::omit;

use super::{
    args::PatArgsId,
    environment::env::{AccessToEnv, WithEnv},
    pats::Spread,
};
use crate::new::{args::ArgsId, params::ParamsId, symbols::Symbol, terms::TermId, tys::TyId};

/// A group of definition parameters
///
/// This is either a set of implicit parameters `<a_1: A_1,...,a_n: A_N>` or a
/// set of explicit parameters `(a_1: A_1,...,a_n: A_N)`.
#[omit(DefParamGroupData, [id], [Debug, Clone, Copy])]
#[derive(Debug, Clone, Copy)]
pub struct DefParamGroup {
    pub id: DefParamGroupId,
    pub implicit: bool,
    pub params: ParamsId,
}
new_sequence_store_key!(pub DefParamsId);
pub type DefParamsStore = DefaultSequenceStore<DefParamsId, DefParamGroup>;
pub type DefParamGroupId = (DefParamsId, usize);

/// A group of definition arguments
///
/// This contains the original parameter group of the definition, as well as
/// set of arguments for it, ordered by the original parameters.
#[omit(DefArgGroupData, [id], [Debug, Clone, Copy])]
#[derive(Debug, Clone, Copy)]
pub struct DefArgGroup {
    pub id: DefArgGroupId,
    pub param_group: DefParamGroupId,
    pub args: ArgsId,
}
new_sequence_store_key!(pub DefArgsId);
pub type DefArgsStore = DefaultSequenceStore<DefArgsId, DefArgGroup>;
pub type DefArgGroupId = (DefArgsId, usize);

/// A group of definition pattern arguments
///
/// This contains the original parameter group of the definition, as well as
/// set of pattern arguments for it, ordered by the original parameters.
#[omit(DefPatArgGroupData, [id], [Debug, Clone, Copy])]
#[derive(Debug, Clone, Copy)]
pub struct DefPatArgGroup {
    pub id: DefPatArgGroupId,
    pub param_group: DefParamGroupId,
    pub pat_args: PatArgsId,
    /// The spread in this group of patterns, if any.
    pub spread: Option<Spread>,
}
new_sequence_store_key!(pub DefPatArgsId);
pub type DefPatArgsStore = DefaultSequenceStore<DefPatArgsId, DefPatArgGroup>;
pub type DefPatArgGroupId = (DefPatArgsId, usize);

/// A member of a definition.
///
/// A definition might be a trait, impl block, or a module.
///
/// Includes a name, the original definition ID, an index into the original
/// definition's members, as well as the type of the member, and an optional
/// value of the member.
#[derive(Debug, Clone, Copy)]
pub struct DefMember<OriginalDefMembersId> {
    pub id: (OriginalDefMembersId, usize),
    pub name: Symbol,
    pub ty: TyId,
    pub value: Option<TermId>,
}

/// The data version of [`DefMember`] (i.e. without ID).
#[derive(Debug, Clone, Copy)]
pub struct DefMemberData {
    pub name: Symbol,
    pub ty: TyId,
    pub value: Option<TermId>,
}

impl Display for WithEnv<'_, &DefParamGroup> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.value.implicit {
            write!(f, "<")?;
        } else {
            write!(f, "(")?;
        }
        write!(f, "{}", self.env().with(self.value.params))?;
        if self.value.implicit {
            write!(f, ">")?;
        } else {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl Display for WithEnv<'_, DefParamGroupId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let param_group = self.stores().def_params().get_element(self.value);
        write!(f, "{}", self.env().with(&param_group))
    }
}

impl Display for WithEnv<'_, DefParamsId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stores().def_params().map_fast(self.value, |params| {
            for param in params.iter() {
                write!(f, "{}", self.env().with(param))?;
            }
            Ok(())
        })
    }
}

impl Display for WithEnv<'_, &DefArgGroup> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let param_group = self.stores().def_params().get_element(self.value.param_group);
        if param_group.implicit {
            write!(f, "<")?;
        } else {
            write!(f, "(")?;
        }
        write!(f, "{}", self.env().with(self.value.args))?;
        if param_group.implicit {
            write!(f, ">")?;
        } else {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl Display for WithEnv<'_, DefArgGroupId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let arg_group = self.stores().def_args().get_element(self.value);
        write!(f, "{}", self.env().with(&arg_group))
    }
}

impl Display for WithEnv<'_, DefArgsId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stores().def_args().map_fast(self.value, |args| {
            for arg in args.iter() {
                write!(f, "{}", self.env().with(arg))?;
            }
            Ok(())
        })
    }
}

impl Display for WithEnv<'_, &DefPatArgGroup> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let param_group = self.stores().def_params().get_element(self.value.param_group);
        if param_group.implicit {
            write!(f, "<")?;
        } else {
            write!(f, "(")?;
        }
        write!(f, "{}", self.env().with(self.value.pat_args))?;

        self.stores().pat_args().map_fast(self.value.pat_args, |pat_args| {
            let mut pat_args_formatted =
                pat_args.iter().map(|arg| self.env().with(arg).to_string()).collect::<Vec<_>>();

            if let Some(Spread { name, index }) = self.value.spread {
                let symbol_data = self.stores().symbol().get(name);
                let name = if let Some(name) = symbol_data.name {
                    name.to_string()
                } else {
                    "".to_string()
                };
                let spread_str = format!("...{}", name);
                pat_args_formatted.insert(index, spread_str);
            }

            for (i, pat_arg) in pat_args_formatted.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{}", pat_arg)?;
            }
            Ok(())
        })?;

        if param_group.implicit {
            write!(f, ">")?;
        } else {
            write!(f, ")")?;
        }
        Ok(())
    }
}

impl Display for WithEnv<'_, DefPatArgGroupId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let pat_arg_group = self.stores().def_pat_args().get_element(self.value);
        write!(f, "{}", self.env().with(&pat_arg_group))
    }
}

impl Display for WithEnv<'_, DefPatArgsId> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.stores().def_pat_args().map_fast(self.value, |pat_args| {
            for pat_arg in pat_args.iter() {
                write!(f, "{}", self.env().with(pat_arg))?;
            }
            Ok(())
        })
    }
}

impl<T> Display for WithEnv<'_, &DefMember<T>> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}{}",
            self.env().with(self.value.name),
            self.env().with(self.value.ty),
            self.value
                .value
                .map(|x| format!(" = {}", self.env().with(x)))
                .unwrap_or_else(|| "".to_string())
        )
    }
}

impl Display for WithEnv<'_, &DefMemberData> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: {}{}",
            self.env().with(self.value.name),
            self.env().with(self.value.ty),
            self.value
                .value
                .map(|x| format!(" = {}", self.env().with(x)))
                .unwrap_or_else(|| "".to_string())
        )
    }
}
