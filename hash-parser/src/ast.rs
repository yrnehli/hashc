//! Frontend-agnostic Hash abstract syntax tree type definitions.
//
// All rights reserved 2021 (c) The Hash Language authors
#![allow(dead_code)]

use crate::{location::Location, modules::ModuleIdx};
use num::BigInt;
use std::hash::Hash;
use std::ops::Deref;

/// Represents an abstract syntax tree node.
///
/// Contains an inner type, as well as begin and end positions in the input.
#[derive(Eq, PartialEq, Hash, Debug, Clone)]
pub struct AstNode<T> {
    /// The actual value contained within this node.
    pub body: Box<T>,
    /// Position of the node in the input.
    pub pos: Location,
    /// Module that this node is part of. Index into [`Modules`](crate::modules::Modules).
    pub module: ModuleIdx,
}

/// [AstNode] hashes as its inner `body` type.
// impl<T: Hash> Hash for AstNode<T> {
//     fn hash<H: Hasher>(&self, state: &mut H) {
//         self.body.hash(state);
//     }
// }

/// [AstNode] dereferences to its inner `body` type.
impl<T> Deref for AstNode<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

/// An intrinsic identifier.
#[derive(Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub struct IntrinsicKey {
    /// The name of the intrinsic (without the "#").
    pub name: &'static str,
}

/// A single name/symbol.
#[derive(Hash, Eq, PartialEq, Copy, Clone, Debug)]
pub struct Name {
    // The name of the symbol.
    pub string: &'static str,
}

/// A namespaced name, i.e. access name.
#[derive(Debug, Clone)]
pub struct AccessName {
    /// The list of names that make up the access name.
    pub names: Vec<AstNode<Name>>,
}

/// A concrete/"named" type.
#[derive(Debug, Clone)]
pub struct NamedType {
    /// The name of the type.
    pub name: AstNode<AccessName>,
    /// The type arguments of the type, if any.
    pub type_args: Vec<AstNode<Type>>,
}

/// A type variable.
#[derive(Debug, Clone)]
pub struct TypeVar {
    /// The name of the type variable.
    pub name: AstNode<Name>,
}

/// A type.
#[derive(Debug, Clone)]
pub enum Type {
    /// A concrete/"named" type.
    Named(NamedType),
    /// A type variable.
    TypeVar(TypeVar),
    /// The existential type (`?`).
    Existential,
    /// The type infer operator.
    Infer,
}

/// A set literal, e.g. `{1, 2, 3}`.
#[derive(Debug, Clone)]
pub struct SetLiteral {
    /// The elements of the set literal.
    pub elements: Vec<AstNode<Expression>>,
}

/// A list literal, e.g. `[1, 2, 3]`.
#[derive(Debug, Clone)]
pub struct ListLiteral {
    /// The elements of the list literal.
    pub elements: Vec<AstNode<Expression>>,
}

/// A tuple literal, e.g. `(1, 'A', "foo")`.
#[derive(Debug, Clone)]
pub struct TupleLiteral {
    /// The elements of the tuple literal.
    pub elements: Vec<AstNode<Expression>>,
}

/// A map literal, e.g. `{"foo": 1, "bar": 2}`.
#[derive(Debug, Clone)]
pub struct MapLiteral {
    /// The elements of the map literal (key-value pairs).
    pub elements: Vec<(AstNode<Expression>, AstNode<Expression>)>,
}

/// A struct literal entry (struct field in struct literal), e.g. `name = "Nani"`.
#[derive(Debug, Clone)]
pub struct StructLiteralEntry {
    /// The name of the struct field.
    pub name: AstNode<Name>,
    /// The value given to the struct field.
    pub value: AstNode<Expression>,
}

/// A struct literal, e.g. `Dog { name = "Adam", age = 12 }`
#[derive(Debug, Clone)]
pub struct StructLiteral {
    /// The name of the struct literal.
    pub name: AstNode<AccessName>,
    /// Type arguments to the struct literal, if any.
    pub type_args: Vec<AstNode<Type>>,
    /// The fields (entries) of the struct literal.
    pub entries: Vec<AstNode<StructLiteralEntry>>,
}

/// A function definition argument.
#[derive(Debug, Clone)]
pub struct FunctionDefArg {
    /// The name of the argument.
    pub name: AstNode<Name>,
    /// The type of the argument, if any.
    ///
    /// Will be inferred if [None].
    pub ty: Option<AstNode<Type>>,
}

/// A function definition.
#[derive(Debug, Clone)]
pub struct FunctionDef {
    /// The arguments of the function definition.
    pub args: Vec<AstNode<FunctionDefArg>>,
    /// The return type of the function definition.
    ///
    /// Will be inferred if [None].
    pub return_ty: Option<AstNode<Type>>,
    /// The body/contents of the function, in the form of an expression.
    pub fn_body: AstNode<Expression>,
}

/// A literal.
#[derive(Debug, Clone)]
pub enum Literal {
    /// A string literal.
    Str(String),
    /// A character literal.
    Char(char),
    /// An integer literal.
    Int(BigInt),
    /// A float literal.
    Float(f64),
    /// A set literal.
    Set(SetLiteral),
    /// A map literal.
    Map(MapLiteral),
    /// A list literal.
    List(ListLiteral),
    /// A tuple literal.
    Tuple(TupleLiteral),
    /// A struct literal.
    Struct(StructLiteral),
    /// A function definition.
    Function(FunctionDef),
}

/// An alternative pattern, e.g. `Red | Blue`.
#[derive(Debug, Clone)]
pub struct OrPattern {
    /// The first pattern in the "or".
    pub a: AstNode<Pattern>,
    /// The second pattern in the "or".
    pub b: AstNode<Pattern>,
}

/// A conditional pattern, e.g. `x if x == 42`.
#[derive(Debug, Clone)]
pub struct IfPattern {
    /// The pattern part of the conditional.
    pub pattern: AstNode<Pattern>,
    /// The expression part of the conditional.
    pub condition: AstNode<Expression>,
}

/// An enum pattern, e.g. `Some((x, y))`.
#[derive(Debug, Clone)]
pub struct EnumPattern {
    /// The name of the enum variant.
    pub name: AstNode<AccessName>,
    /// The arguments of the enum variant as patterns.
    pub args: Vec<AstNode<Pattern>>,
}

/// A pattern destructuring, e.g. `name: (fst, snd)`.
///
/// Used in struct and namespace patterns.
#[derive(Debug, Clone)]
pub struct DestructuringPattern {
    /// The name of the field.
    pub name: AstNode<Name>,
    /// The pattern to match the field's value with.
    pub patterns: AstNode<Pattern>,
}

/// A struct pattern, e.g. `Dog { name = "Frank"; age; }`
#[derive(Debug, Clone)]
pub struct StructPattern {
    /// The name of the struct.
    pub name: AstNode<AccessName>,
    /// The entries of the struct, as [DestructuringPattern] entries.
    pub entries: Vec<AstNode<DestructuringPattern>>,
}

/// A namespace pattern, e.g. `{ fgets; fputs; }`
#[derive(Debug, Clone)]
pub struct NamespacePattern {
    /// The entries of the namespace, as [DestructuringPattern] entries.
    pub patterns: Vec<AstNode<DestructuringPattern>>,
}

/// A tuple pattern, e.g. `(1, 2, x)`
#[derive(Debug, Clone)]
pub struct TuplePattern {
    /// The element of the tuple, as patterns.
    pub elements: Vec<AstNode<Pattern>>,
}

/// A literal pattern, e.g. `1`, `3.4`, `"foo"`.
#[derive(Debug, Clone)]
pub enum LiteralPattern {
    /// A string literal pattern.
    Str(String),
    /// A character literal pattern.
    Char(char),
    /// An integer literal pattern.
    Int(BigInt),
    /// A float literal pattern.
    Float(f64),
}

/// A pattern. e.g. `Ok(Dog {props = (1, x)})`.
#[derive(Debug, Clone)]
pub enum Pattern {
    /// An enum pattern.
    Enum(EnumPattern),
    /// A struct pattern.
    Struct(StructPattern),
    /// A namespace pattern.
    Namespace(NamespacePattern),
    /// A tuple pattern.
    Tuple(TuplePattern),
    /// A literal pattern.
    Literal(LiteralPattern),
    /// An alternative/"or" pattern.
    Or(OrPattern),
    /// A conditional/"if" pattern.
    If(IfPattern),
    /// A pattern name binding.
    Binding(AstNode<Name>),
    /// The catch-all, i.e "ignore" pattern.
    Ignore,
}

/// A trait bound, e.g. "where eq<T>"
#[derive(Debug, Clone)]
pub struct TraitBound {
    /// The name of the trait.
    pub name: AstNode<AccessName>,
    /// The type arguments of the trait.
    pub type_args: Vec<AstNode<Type>>,
}

/// A bound, e.g. "<T, U> where conv<U, T>".
///
/// Used in struct, enum, trait definitions.
#[derive(Debug, Clone)]
pub struct Bound {
    /// The type arguments of the bound.
    pub type_args: Vec<AstNode<Type>>,
    /// The traits that constrain the bound, if any.
    pub trait_bounds: Vec<AstNode<TraitBound>>,
}

/// A let statement, e.g. `let x = 3;`.
#[derive(Debug, Clone)]
pub struct LetStatement {
    /// The pattern to bind the right-hand side to.
    pub pattern: AstNode<Pattern>,
    /// The bound of the let, if any.
    ///
    /// Used for trait implementations.
    pub bound: Option<AstNode<Bound>>,
}

/// An assign statement, e.g. `x = 4;`.
#[derive(Debug, Clone)]
pub struct AssignStatement {
    /// The left-hand side of the assignment.
    ///
    /// This should resolve to either a variable or a struct field.
    pub lhs: AstNode<Expression>,
    /// The right-hand side of the assignment.
    ///
    /// The value will be assigned to the left-hand side.
    pub rhs: AstNode<Expression>,
}

/// A field of a struct definition, e.g. "name: str".
#[derive(Debug, Clone)]
pub struct StructDefEntry {
    /// The name of the struct field.
    pub name: AstNode<Name>,
    /// The type of the struct field.
    ///
    /// Will be inferred if [None].
    pub ty: Option<AstNode<Type>>,
    /// The default value of the struct field, if any.
    pub default: Option<AstNode<Expression>>,
}

/// A struct definition, e.g. `struct Foo = { bar: int; };`.
#[derive(Debug, Clone)]
pub struct StructDef {
    /// The name of the struct.
    pub name: AstNode<Name>,
    /// The bound of the struct.
    pub bound: AstNode<Bound>,
    /// The fields of the struct, in the form of [StructDefEntry].
    pub entries: Vec<AstNode<StructDefEntry>>,
}

/// A variant of an enum definition, e.g. `Some(T)`.
#[derive(Debug, Clone)]
pub struct EnumDefEntry {
    /// The name of the enum variant.
    pub name: AstNode<Name>,
    /// The arguments of the enum variant, if any.
    pub args: Vec<AstNode<Type>>,
}

/// An enum definition, e.g. `enum Option = <T> => { Some(T); None; };`.
#[derive(Debug, Clone)]
pub struct EnumDef {
    /// The name of the enum.
    pub name: AstNode<Name>,
    /// The bounds of the enum.
    pub bound: AstNode<Bound>,
    /// The variants of the enum, in the form of [EnumDefEntry].
    pub entries: Vec<AstNode<EnumDefEntry>>,
}

/// A trait definition, e.g. `trait add = <T> => (T, T) => T;`.
#[derive(Debug, Clone)]
pub struct TraitDef {
    /// The name of the trait.
    pub name: AstNode<Name>,
    /// The bound of the trait.
    pub bound: AstNode<Bound>,
    /// The inner type of the trait. Expected to be a `Function` type.
    pub trait_type: AstNode<Type>,
}

/// A statement.
#[derive(Debug, Clone)]
pub enum Statement {
    /// An expression statement, e.g. `my_func();`
    Expr(AstNode<Expression>),
    /// An return statement.
    ///
    /// Has an optional return expression, which becomes `void` if [None] is given.
    Return(Option<AstNode<Expression>>),
    /// A block statement.
    Block(AstNode<Block>),
    /// Break statement (only in loop context).
    Break,
    /// Continue statement (only in loop context).
    Continue,
    /// A let statement.
    Let(LetStatement),
    /// An assign statement.
    Assign(AssignStatement),
    /// A struct definition.
    StructDef(StructDef),
    /// An enum definition.
    EnumDef(EnumDef),
    /// A trait definition.
    TraitDef(TraitDef),
}

/// A branch/"case" of a `match` block.
#[derive(Debug, Clone)]
pub struct MatchCase {
    /// The pattern of the `match` case.
    pub pattern: AstNode<Pattern>,
    /// The expression corresponding to the match case.
    ///
    /// Will be executed if the pattern succeeeds.
    pub expr: AstNode<Expression>,
}

/// A `match` block.
#[derive(Debug, Clone)]
pub struct MatchBlock {
    /// The expression to match on.
    pub subject: AstNode<Expression>,
    /// The match cases to execute.
    pub cases: Vec<AstNode<MatchCase>>,
}

/// A body block.
#[derive(Debug, Clone)]
pub struct BodyBlock {
    /// Zero or more statements.
    pub statements: Vec<AstNode<Statement>>,
    /// Zero or one expression.
    pub expr: Option<AstNode<Expression>>,
}

/// A block.
#[derive(Debug, Clone)]
pub enum Block {
    /// A match block.
    Match(MatchBlock),
    /// A loop block.
    ///
    /// The inner block is the loop body.
    Loop(AstNode<Block>),
    /// A body block.
    Body(BodyBlock),
}

/// Function call arguments.
#[derive(Debug, Clone)]
pub struct FunctionCallArgs {
    /// Each argument of the function call, as an expression.
    pub entries: Vec<AstNode<Expression>>,
}

/// A function call expression.
#[derive(Debug, Clone)]
pub struct FunctionCallExpr {
    /// An expression which evaluates to a function value.
    pub subject: AstNode<Expression>,
    /// Arguments to the function, in the form of [FunctionCallArgs].
    pub args: AstNode<FunctionCallArgs>,
}

/// A logical operator.
///
/// These are treated differently from all other operators due to short-circuiting.
#[derive(Debug, Clone, Eq, PartialEq, Copy)]
pub enum LogicalOp {
    /// The logical-and operator.
    And,
    /// The logical-or operator.
    Or,
}

/// A logical operation expression.
#[derive(Debug, Clone)]
pub struct LogicalOpExpr {
    /// The operator of the logical operation.
    pub op: AstNode<LogicalOp>,
    /// The left-hand side of the operation.
    pub lhs: AstNode<Expression>,
    /// The right-hand side of the operation.
    pub rhs: AstNode<Expression>,
}

/// A property access exprssion.
#[derive(Debug, Clone)]
pub struct PropertyAccessExpr {
    /// An expression which evaluates to a struct or tuple value.
    pub subject: AstNode<Expression>,
    /// The property of the subject to access.
    pub property: AstNode<Name>,
}

/// A typed expression, e.g. `foo as int`.
#[derive(Debug, Clone)]
pub struct TypedExpr {
    /// The annotated type of the expression.
    pub ty: AstNode<Type>,
    /// The expression being typed.
    pub expr: AstNode<Expression>,
}

/// Represents a path to a module, given as a string literal to an `import` call.
type ImportPath = String;

/// A variable expression.
#[derive(Debug, Clone)]
pub struct VariableExpr {
    /// The name of the variable.
    pub name: AstNode<AccessName>,
    /// Any type arguments of the variable. Only valid for traits.
    pub type_args: Vec<AstNode<Type>>,
}

/// An expression.
#[derive(Debug, Clone)]
pub enum Expression {
    /// A function call.
    FunctionCall(FunctionCallExpr),
    /// An intrinsic symbol.
    Intrinsic(IntrinsicKey),
    /// A logical operation.
    LogicalOp(LogicalOpExpr),
    /// A variable.
    Variable(VariableExpr),
    /// A property access.
    PropertyAccess(PropertyAccessExpr),
    /// A literal.
    LiteralExpr(Literal),
    /// A typed expression.
    Typed(TypedExpr),
    /// A block.
    Block(AstNode<Block>),
    /// An `import` call.
    Import(AstNode<ImportPath>),
}

/// A module.
///
/// Represents a parsed `.hash` file.
#[derive(Debug, Clone)]
pub struct Module {
    /// The contents of the module, as a list of statements.
    pub contents: Vec<AstNode<Statement>>,
}