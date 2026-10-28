//! The syntax tree of the Perl subset ExifTool's tables, its eval sites and
//! `.ExifTool_config` files are written in.

use std::rc::Rc;

use crate::perl::Value;

pub type P<T> = Box<T>;
pub type Block = Vec<Stmt>;

/// Where an array or hash lives: a named variable or a dereference.
#[derive(Debug, Clone)]
pub enum Container {
    /// `@name` / `%name` (the name without sigil).
    Named(Rc<str>),
    /// `@{EXPR}`, `@$x`, `$x->[..]`: the expression yields the reference.
    Deref(P<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Repeat,
    NumEq,
    NumNe,
    NumLt,
    NumGt,
    NumLe,
    NumGe,
    NumCmp,
    StrEq,
    StrNe,
    StrLt,
    StrGt,
    StrLe,
    StrGe,
    StrCmp,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    /// `&&` / `and`
    And,
    /// `||` / `or`
    Or,
    /// `//`
    DefOr,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Not,
    Neg,
    Plus,
    BitNot,
    /// `\EXPR`
    Ref,
    PreInc,
    PreDec,
    PostInc,
    PostDec,
}

/// Case-modifying escapes of interpolated strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaseOp {
    Upper,      // \U
    Lower,      // \L
    Quote,      // \Q
    End,        // \E
    UpperNext,  // \u
    LowerNext,  // \l
}

/// A piece of an interpolated string or pattern.
#[derive(Debug, Clone)]
pub enum Part {
    Lit(Vec<u8>),
    /// A scalar expression, stringified.
    Scalar(Expr),
    /// An array expression, joined with `$"`.
    Array(Expr),
    Case(CaseOp),
}

/// A pattern: constant, or interpolated at run time.
#[derive(Debug, Clone)]
pub struct Pat {
    pub parts: Vec<Part>,
    pub flags: Rc<str>,
}

impl Pat {
    pub fn constant(&self) -> Option<&[u8]> {
        match self.parts.as_slice() {
            [] => Some(b""),
            [Part::Lit(b)] => Some(b),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum MethodName {
    Static(Rc<str>),
    Dynamic(P<Expr>),
}

/// The first argument of `map`, `grep` and `sort`.
#[derive(Debug, Clone)]
pub enum Callback {
    Block(Rc<Block>),
    Expr(P<Expr>),
}

#[derive(Debug, Clone)]
pub struct Sub {
    pub name: Option<Rc<str>>,
    pub package: Rc<str>,
    pub body: Rc<Block>,
}

#[derive(Debug, Clone)]
pub enum Expr {
    Const(Value),
    Interp(Vec<Part>),
    /// `$name`
    Scalar(Rc<str>),
    /// `$$x`, `${EXPR}`
    ScalarDeref(P<Expr>),
    /// `@name`, `@$x`
    Array(Container),
    /// `%name`, `%$x`
    Hash(Container),
    /// `$#name`, `$#$x`
    LastIndex(Container),
    /// `$a[i]`, `$$r[i]`, `$r->[i]`
    Elem(Container, P<Expr>),
    /// `$h{k}`, `$$r{k}`, `$r->{k}`
    HElem(Container, P<Expr>),
    /// `@a[LIST]`
    Slice(Container, P<Expr>),
    /// `@h{LIST}`
    HSlice(Container, P<Expr>),
    /// `(LIST)[LIST]`
    ListSlice(P<Expr>, P<Expr>),
    /// `my $x`, `my ($a, @b)`: the declared variables; `paren` for the list form.
    My(Vec<Expr>, bool),
    Local(P<Expr>),
    Assign(Option<BinOp>, P<Expr>, P<Expr>),
    Bin(BinOp, P<Expr>, P<Expr>),
    Un(UnOp, P<Expr>),
    Cond(P<Expr>, P<Expr>, P<Expr>),
    Range(P<Expr>, P<Expr>),
    List(Vec<Expr>),
    AnonArray(Vec<Expr>),
    AnonHash(Vec<Expr>),
    AnonSub(Rc<Sub>),
    /// A builtin or a named sub. `amp` for `&name(...)`; `parens` when the
    /// arguments were in parentheses (matters for `print (...)`-like cases).
    Call { name: Rc<str>, args: Vec<Expr>, amp: bool },
    /// `$code->(ARGS)`, `&$code(ARGS)`, `&{...}(ARGS)`
    CallRef(P<Expr>, Vec<Expr>),
    Method { inv: P<Expr>, name: MethodName, args: Vec<Expr> },
    Match { target: Option<P<Expr>>, neg: bool, pat: Pat },
    Subst { target: Option<P<Expr>>, pat: Pat, repl: SubstRepl },
    Trans { target: Option<P<Expr>>, search: Vec<u8>, repl: Vec<u8>, flags: Rc<str> },
    Qr(Pat),
    Do(Rc<Block>),
    Eval(Rc<Block>),
    EvalStr(P<Expr>),
    Map(Callback, P<Expr>),
    Grep(Callback, P<Expr>),
    Sort(Option<Callback>, P<Expr>),
    FileTest(u8, P<Expr>),
    Return(Option<P<Expr>>),
    Last(Option<Rc<str>>),
    Next(Option<Rc<str>>),
    Redo(Option<Rc<str>>),
    Goto(Rc<str>),
    Wantarray,
    /// `print`/`printf` with an optional filehandle: a bareword is a
    /// `Const("*NAME")`, otherwise the expression holding the handle.
    Print { printf: bool, fh: Option<P<Expr>>, args: Vec<Expr> },
    /// `<FH>`, `<$fh>`
    ReadLine(P<Expr>),
}

#[derive(Debug, Clone)]
pub enum SubstRepl {
    Interp(Vec<Part>),
    /// `/e`: the replacement is code.
    Code(Rc<Block>),
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Expr(Expr),
    If(Vec<(Expr, Block)>, Option<Block>),
    While { label: Option<Rc<str>>, cond: Option<Expr>, body: Block, until: bool, post: bool },
    ForC { label: Option<Rc<str>>, init: Option<Expr>, cond: Option<Expr>, step: Option<Expr>, body: Block },
    /// `foreach [my] $var (LIST) BLOCK`; `var` is `None` for `$_`.
    Foreach { label: Option<Rc<str>>, var: Option<(bool, Rc<str>)>, list: Expr, body: Block },
    Block(Option<Rc<str>>, Block),
    SubDef(Rc<Sub>),
    Package(Rc<str>, Option<Block>),
    /// `use`/`no`: kept for `use vars`/`use constant`-like effects.
    Use(Rc<str>, Option<Expr>),
    /// `END { ... }`: run when the program ends.
    End(Rc<Block>),
    /// `LABEL:` before a statement that is not a loop (a `goto` target).
    Label(Rc<str>),
    /// `EXPR if COND` / `EXPR unless COND`: no scope of its own, so
    /// `my $x = ... if ...` declares in the enclosing block.
    Modified(Expr, Expr),
    /// The line the next statement starts on (for die/warn locations).
    Line(u32),
    /// `use warnings` / `no warnings` (lexical): whether warnings are on
    /// from here to the end of the block.
    Warnings(Option<bool>),
}
