//! Store to keep track of all the directives in the program, and their targets.

use derive_more::From;
use hash_source::identifier::Identifier;
use hash_storage::store::DefaultPartialStore;
use indexmap::IndexSet;

use crate::{
    data::{CtorDefId, DataDefId},
    fns::FnDefId,
    mods::{ModDefId, ModMemberValue},
    params::ParamId,
    pats::PatId,
    terms::TermId,
    tys::TyId,
};

macro_rules! directive_targets {
    ($($name:ident),* $(,)?) => {
        /// All the atoms in the TIR which are valid targets for directives.
        #[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, From)]
        pub enum DirectiveTarget {
           $(
               $name($name),
           )*
        }
    };
}
directive_targets! {
    TermId,
    TyId,
    PatId,
    ParamId,
    FnDefId,
    DataDefId,
    ModDefId,
    CtorDefId,
}

impl From<ModMemberValue> for DirectiveTarget {
    fn from(value: ModMemberValue) -> Self {
        match value {
            ModMemberValue::Fn(fn_def) => Self::FnDefId(fn_def),
            ModMemberValue::Data(data_def) => Self::DataDefId(data_def),
            ModMemberValue::Mod(mod_def) => Self::ModDefId(mod_def),
        }
    }
}

/// A set of directives that have been applied to a target.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppliedDirectives {
    pub directives: IndexSet<Identifier>,
}

impl AppliedDirectives {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check whether a directive is being applied to this target.
    pub fn contains(&self, directive: Identifier) -> bool {
        self.directives.contains(&directive)
    }

    /// Iterate over all discovered functions.
    pub fn iter(&self) -> impl Iterator<Item = Identifier> + '_ {
        self.directives.iter().copied()
    }
}

pub type AppliedDirectivesStore = DefaultPartialStore<DirectiveTarget, AppliedDirectives>;

impl std::fmt::Display for DirectiveTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DirectiveTarget::TermId(term) => write!(f, "{}", term),
            DirectiveTarget::TyId(ty) => write!(f, "{}", ty),
            DirectiveTarget::PatId(pat) => write!(f, "{}", pat),
            DirectiveTarget::ParamId(param) => {
                write!(f, "{}", param)
            }
            DirectiveTarget::FnDefId(fn_def) => write!(f, "{}", fn_def),
            DirectiveTarget::DataDefId(data_def) => {
                write!(f, "{}", data_def)
            }
            DirectiveTarget::ModDefId(mod_def) => {
                write!(f, "{}", mod_def)
            }
            DirectiveTarget::CtorDefId(ctor_def) => {
                write!(f, "{}", ctor_def)
            }
        }
    }
}
