//! Abstract syntax tree types for Karn v0 (spec §9.2).

use crate::span::Span;

/// An identifier with its source span.
#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

/// A whole parsed commons source file.
#[derive(Debug, Clone)]
pub struct Commons {
    pub name: QualifiedName,
    pub items: Vec<CommonsItem>,
    pub span: Span,
}

/// A dotted name like `fitness.units`.
#[derive(Debug, Clone)]
pub struct QualifiedName {
    pub parts: Vec<Ident>,
    pub span: Span,
}

impl QualifiedName {
    pub fn joined(&self) -> String {
        self.parts
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>()
            .join(".")
    }
}

#[derive(Debug, Clone)]
pub enum CommonsItem {
    Type(TypeDecl),
    Fn(FnDecl),
}

impl CommonsItem {
    pub fn name(&self) -> &Ident {
        match self {
            CommonsItem::Type(t) => &t.name,
            CommonsItem::Fn(f) => &f.name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TypeDecl {
    pub name: Ident,
    pub base: BaseType,
    pub base_span: Span,
    pub refinement: Option<Refinement>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaseType {
    Int,
    String,
    Bool,
}

impl BaseType {
    pub fn name(self) -> &'static str {
        match self {
            BaseType::Int => "Int",
            BaseType::String => "String",
            BaseType::Bool => "Bool",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Refinement {
    pub predicates: Vec<RefinementPred>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct RefinementPred {
    pub kind: PredKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PredKind {
    Matches(String),
    InRange(i64, i64),
    MinLength(i64),
    MaxLength(i64),
    Length(i64),
    NonNegative,
    Positive,
    NonEmpty,
}

impl PredKind {
    pub fn name(&self) -> &'static str {
        match self {
            PredKind::Matches(_) => "Matches",
            PredKind::InRange(..) => "InRange",
            PredKind::MinLength(_) => "MinLength",
            PredKind::MaxLength(_) => "MaxLength",
            PredKind::Length(_) => "Length",
            PredKind::NonNegative => "NonNegative",
            PredKind::Positive => "Positive",
            PredKind::NonEmpty => "NonEmpty",
        }
    }
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    pub name: Ident,
    pub params: Vec<Param>,
    pub return_type: TypeRef,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Param {
    pub name: Ident,
    pub type_ref: TypeRef,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum TypeRef {
    Base(BaseType, Span),
    Named(Ident),
}

impl TypeRef {
    pub fn span(&self) -> Span {
        match self {
            TypeRef::Base(_, s) => *s,
            TypeRef::Named(id) => id.span,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    IntLit(i64),
    StrLit(String),
    BoolLit(bool),
    Ident(Ident),
    Call(Ident, Vec<Expr>),
    BinOp(BinOp, Box<Expr>, Box<Expr>),
    UnaryOp(UnaryOp, Box<Expr>),
    Paren(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Or,
    And,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Add,
    Sub,
    Mul,
    Div,
}

impl BinOp {
    pub fn name(self) -> &'static str {
        match self {
            BinOp::Or => "||",
            BinOp::And => "&&",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::LtEq => "<=",
            BinOp::Gt => ">",
            BinOp::GtEq => ">=",
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

impl UnaryOp {
    pub fn name(self) -> &'static str {
        match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
        }
    }
}
