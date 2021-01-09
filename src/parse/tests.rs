#![allow(non_snake_case)]

use std::fmt::Debug;

use super::parse;
use crate::color::RgbaColor;
use crate::diag::{Diag, Level, Pass};
use crate::geom::LengthUnit;
use crate::syntax::*;

use BinOp::*;
use Expr::{Bool, Color, Float, Int, Length, Percent};
use Node::{Emph, Expr as Block, Linebreak, Parbreak, Space, Strong};
use UnOp::*;

macro_rules! t {
    ($src:literal
        nodes: [$($node:expr),* $(,)?]
        $(, errors: [$($err:expr),* $(,)?])?
        $(, warnings: [$($warn:expr),* $(,)?])?
        $(, spans: $spans:expr)?
        $(,)?
    ) => {{
        #[allow(unused)]
        let mut spans = false;
        $(spans = $spans;)?

        let Pass { output, feedback } = parse($src);
        check($src, Content![@$($node),*], output, spans);
        check(
            $src,
            vec![
                $($(into!($err).map(|s: &str| Diag::new(Level::Error, s)),)*)?
                $($(into!($warn).map(|s: &str| Diag::new(Level::Warning, s)),)*)?
            ],
            feedback.diags,
            true,
        );
    }};

    ($src:literal $($node:expr),* $(,)?) => {
        t!($src nodes: [$($node),*]);
    };
}

/// Assert that expected and found are equal, printing both and the source of
/// the test case if they aren't.
///
/// When `cmp_spans` is false, spans are ignored.
#[track_caller]
pub fn check<T>(src: &str, exp: T, found: T, cmp_spans: bool)
where
    T: Debug + PartialEq,
{
    Span::set_cmp(cmp_spans);

    if exp != found {
        println!("source:   {:?}", src);
        println!("expected: {:#?}", exp);
        println!("found:    {:#?}", found);
        panic!("test failed");
    }

    Span::set_cmp(true);
}

/// Shorthand for `Spanned::new`.
fn S<T>(span: impl Into<Span>, v: T) -> Spanned<T> {
    Spanned::new(v, span)
}

// Enables tests to optionally specify spans.
impl<T> From<T> for Spanned<T> {
    fn from(t: T) -> Self {
        Spanned::zero(t)
    }
}

/// Shorthand for `Into::<Spanned<_>>::into`.
macro_rules! into {
    ($val:expr) => {
        Into::<Spanned<_>>::into($val)
    };
}

fn Text(text: &str) -> Node {
    Node::Text(text.into())
}

fn Heading(level: impl Into<Spanned<u8>>, contents: Tree) -> Node {
    Node::Heading(NodeHeading { level: level.into(), contents })
}

fn Raw(lang: Option<&str>, lines: &[&str], inline: bool) -> Node {
    Node::Raw(NodeRaw {
        lang: lang.map(|id| Ident(id.into())),
        lines: lines.iter().map(ToString::to_string).collect(),
        inline,
    })
}

fn Id(ident: &str) -> Expr {
    Expr::Ident(Ident(ident.to_string()))
}

fn Str(string: &str) -> Expr {
    Expr::Str(string.to_string())
}

fn Binary(
    lhs: impl Into<Spanned<Expr>>,
    op: impl Into<Spanned<BinOp>>,
    rhs: impl Into<Spanned<Expr>>,
) -> Expr {
    Expr::Binary(ExprBinary {
        lhs: Box::new(lhs.into()),
        op: op.into(),
        rhs: Box::new(rhs.into()),
    })
}

fn Unary(op: impl Into<Spanned<UnOp>>, expr: impl Into<Spanned<Expr>>) -> Expr {
    Expr::Unary(ExprUnary {
        op: op.into(),
        expr: Box::new(expr.into()),
    })
}

macro_rules! Array {
    (@$($expr:expr),* $(,)?) => {
        vec![$(into!($expr)),*]
    };
    ($($tts:tt)*) => (Expr::Array(Array![@$($tts)*]));
}

macro_rules! Dict {
    (@$($name:expr => $expr:expr),* $(,)?) => {
        vec![$(Named {
            name: into!($name).map(|s: &str| Ident(s.into())),
            expr: into!($expr)
        }),*]
    };
    ($($tts:tt)*) => (Expr::Dict(Dict![@$($tts)*]));
}

macro_rules! Args {
    (@$a:expr) => {
        Argument::Pos(into!($a))
    };
    (@$a:expr => $b:expr) => {
        Argument::Named(Named {
            name: into!($a).map(|s: &str| Ident(s.into())),
            expr: into!($b)
        })
    };
    ($($a:expr $(=> $b:expr)?),* $(,)?) => {
        vec![$(Args!(@$a $(=> $b)?)),*]
    };
}

macro_rules! Content {
    (@$($node:expr),* $(,)?) => (vec![$(into!($node)),*]);
    ($($tts:tt)*) => (Expr::Content(Content![@$($tts)*]));
}

macro_rules! Call {
    (@@$name:expr) => {
        Call!(@@$name, Args![])
    };
    (@@$name:expr, $args:expr) => {
        ExprCall {
            name: into!($name).map(|s: &str| Ident(s.into())),
            args: into!($args),
        }
    };
    (@$($tts:tt)*) => (Expr::Call(Call!(@@$($tts)*)));
    ($($tts:tt)*) => (Node::Expr(Call!(@$($tts)*)));
}

#[test]
fn test_parse_comments() {
    // In body.
    t!("a// you\nb"        Text("a"), Space, Text("b"));
    t!("* // \n /*\n\n*/*" Strong, Space, Space, Strong);

    // In header.
    t!("[v /*12pt*/]"            Call!("v"));
    t!("[v //\n]"                Call!("v"));
    t!("[v 12, /*\n*/ size: 14]" Call!("v", Args![Int(12), "size" => Int(14)]));

    // Error.
    t!("a*/b"
        nodes: [Text("a"), Text("b")],
        errors: [S(1..3, "unexpected end of block comment")]);
}

#[test]
fn test_parse_simple_nodes() {
    // Basics.
    t!("");
    t!(" "    Space);
    t!("hi"   Text("hi"));
    t!("🧽"   Text("🧽"));
    t!("_"    Emph);
    t!("*"    Strong);
    t!("~"    Text("\u{00A0}"));
    t!(r"\"   Linebreak);
    t!("\n\n" Parbreak);

    // Multiple nodes.
    t!("ab c"         Text("ab"), Space, Text("c"));
    t!("a`hi`\r\n\r*" Text("a"), Raw(None, &["hi"], true), Parbreak, Strong);

    // Spans.
    t!("*🌍*"
        nodes: [S(0..1, Strong), S(1..5, Text("🌍")), S(5..6, Strong)],
        spans: true);

    // Errors.
    t!("]}"
        nodes: [],
        errors: [S(0..1, "unexpected closing bracket"),
                 S(1..2, "unexpected closing brace")]);
}

#[test]
fn test_parse_headings() {
    // Basics with spans.
    t!("#a"
        nodes: [S(0..2, Heading(S(0..1, 0), Content![@S(1..2, Text("a"))]))],
        spans: true);

    // Multiple hashtags.
    t!("###three"   Heading(2, Content![@Text("three")]));
    t!("###### six" Heading(5, Content![@Space, Text("six")]));

    // Start of heading.
    t!("/**/#"    Heading(0, Content![@]));
    t!("[f][#ok]" Call!("f", Args![Content![Heading(0, Content![@Text("ok")])]]));

    // End of heading.
    t!("#a\nb" Heading(0, Content![@Text("a")]), Space, Text("b"));

    // Continued heading.
    t!("#a{\n1\n}b"   Heading(0, Content![@Text("a"), Block(Int(1)), Text("b")]));
    t!("#a[f][\n\n]d" Heading(0, Content![@
        Text("a"), Call!("f", Args![Content![Parbreak]]), Text("d"),
    ]));

    // No heading.
    t!(r"\#"    Text("#"));
    t!("Nr. #1" Text("Nr."), Space, Text("#"), Text("1"));
    t!("[v]#"   Call!("v"), Text("#"));

    // Too many hashtags.
    t!("####### seven"
        nodes: [Heading(5, Content![@Space, Text("seven")])],
        warnings: [S(0..7, "section depth should not exceed 6")]);
}

#[test]
fn test_parse_raw() {
    // Basic, mostly tested in tokenizer and resolver.
    t!("`py`" nodes: [S(0..4, Raw(None, &["py"], true))], spans: true);
    t!("`endless"
        nodes: [Raw(None, &["endless"], true)],
        errors: [S(8..8, "expected backtick(s)")]);
}

#[test]
fn test_parse_escape_sequences() {
    // Basic, mostly tested in tokenizer.
    t!(r"\[" Text("["));
    t!(r"\u{1F3D5}" nodes: [S(0..9, Text("🏕"))], spans: true);

    // Bad value.
    t!(r"\u{FFFFFF}"
        nodes: [Text(r"\u{FFFFFF}")],
        errors: [S(0..10, "invalid unicode escape sequence")]);

    // No closing brace.
    t!(r"\u{41*"
        nodes: [Text("A"), Strong],
        errors: [S(5..5, "expected closing brace")]);
}

#[test]
fn test_parse_groups() {
    // Test paren group.
    t!("{([v 1) + 3}"
        nodes: [Block(Binary(
            Content![Call!("v", Args![Int(1)])],
            Add,
            Int(3),
        ))],
        errors: [S(6..6, "expected closing bracket")]);

    // Test bracket group.
    t!("[)"
        nodes: [Call!("")],
        errors: [S(1..2, "expected function name, found closing paren"),
                 S(2..2, "expected closing bracket")]);

    t!("[v {*]_"
        nodes: [Call!("v", Args![Content![Strong]]), Emph],
        errors: [S(5..5, "expected closing brace")]);

    // Test brace group.
    t!("{1 + [}"
        nodes: [Block(Binary(Int(1), Add, Content![Call!("")]))],
        errors: [S(6..6, "expected function name"),
                 S(6..6, "expected closing bracket")]);

    // Test subheader group.
    t!("[v (|u )]"
        nodes: [Call!("v", Args![Array![], Content![Call!("u")]])],
        errors: [S(4..4, "expected closing paren"),
                 S(7..8, "expected expression, found closing paren")]);
}

#[test]
fn test_parse_blocks() {
    // Basic with spans.
    t!("{1}" nodes: [S(0..3, Block(Int(1)))], spans: true);

    // Function calls.
    t!("{f()}" Call!("f"));
    t!("{[f]}" Block(Content![Call!("f")]));

    // Missing or bad value.
    t!("{}{1u}"
        nodes: [],
        errors: [S(1..1, "expected expression"),
                 S(3..5, "expected expression, found invalid token")]);

    // Too much stuff.
    t!("{1 #{} end"
        nodes: [Block(Int(1)), Space, Text("end")],
        errors: [S(3..4, "unexpected hex value"),
                 S(4..5, "unexpected opening brace")]);
}

#[test]
fn test_parse_bracket_funcs() {
    // Basic.
    t!("[function]" Call!("function"));
    t!("[ v ]"      Call!("v"));

    // Body and no body.
    t!("[v][[f]]"  Call!("v", Args![Content![Call!("f")]]));
    t!("[v][v][v]" Call!("v", Args![Content![Text("v")]]), Call!("v"));
    t!("[v] [f]"   Call!("v"), Space, Call!("f"));

    // Spans.
    t!("[v 1][📐]"
        nodes: [S(0..11, Call!(S(1..2, "v"), S(3..4, Args![
            S(3..4, Int(1)),
            S(5..11, Content![S(6..10, Text("📐"))]),
        ])))],
        spans: true);

    // No name and no closing bracket.
    t!("["
        nodes: [Call!("")],
        errors: [S(1..1, "expected function name"),
                 S(1..1, "expected closing bracket")]);

    // No name.
    t!("[]"
        nodes: [Call!("")],
        errors: [S(1..1, "expected function name")]);

    // Bad name.
    t!("[# 1]"
        nodes: [Call!("", Args![Int(1)])],
        errors: [S(1..2, "expected function name, found hex value")]);

    // String in header eats closing bracket.
    t!(r#"[v "]"#
        nodes: [Call!("v", Args![Str("]")])],
        errors: [S(5..5, "expected quote"),
                 S(5..5, "expected closing bracket")]);

    // Raw in body eats closing bracket.
    t!("[v][`a]`"
        nodes: [Call!("v", Args![Content![Raw(None, &["a]"], true)]])],
        errors: [S(8..8, "expected closing bracket")]);
}

#[test]
fn test_parse_chaining() {
    // Basic.
    t!("[a | b]" Call!("a", Args![Content![Call!("b")]]));
    t!("[a|b|c]" Call!("a", Args![Content![
        Call!("b", Args![Content![Call!("c")]])
    ]]));

    // With body and spans.
    t!("[a|b][💕]"
        nodes: [S(0..11, Call!(S(1..2, "a"), S(2..2, Args![
            S(3..11, Content![S(3..11, Call!(S(3..4, "b"), S(4..4, Args![
                S(5..11, Content![S(6..10, Text("💕"))])
            ])))])
        ])))],
        spans: true);

    // No name in second subheader.
    t!("[a 1|]"
        nodes: [Call!("a", Args![Int(1), Content![Call!("")]])],
        errors: [S(5..5, "expected function name")]);

    // No name in first subheader.
    t!("[|a true]"
        nodes: [Call!("", Args![Content![Call!("a", Args![Bool(true)])]])],
        errors: [S(1..1, "expected function name")]);
}

#[test]
fn test_parse_arguments() {
    // Bracket functions.
    t!("[v a]"   Call!("v", Args![Id("a")]));
    t!("[v 1,]"  Call!("v", Args![Int(1)]));
    t!("[v a:2]" Call!("v", Args!["a" => Int(2)]));

    // Parenthesized function with nested array literal.
    t!(r#"{f(1, a: (2, 3), #004, b: "five")}"# Block(Call!(@"f", Args![
        Int(1),
        "a" => Array![Int(2), Int(3)],
        Color(RgbaColor::new(0, 0, 0x44, 0xff)),
        "b" => Str("five"),
    ])));

    // Bad expression.
    t!("[v */]"
        nodes: [Call!("v", Args![])],
        errors: [S(3..5, "expected expression, found end of block comment")]);

    // Bad expression.
    t!("[v a:1:]"
        nodes: [Call!("v", Args!["a" => Int(1)])],
        errors: [S(6..7, "expected expression, found colon")]);

    // Missing comma between arguments.
    t!("[v 1 2]"
        nodes: [Call!("v", Args![Int(1), Int(2)])],
        errors: [S(4..4, "expected comma")]);

    // Name has to be identifier.
    t!("[v 1:]"
        nodes: [Call!("v", Args![])],
        errors: [S(3..4, "name must be identifier"),
                 S(5..5, "expected expression")]);

    // Name has to be identifier.
    t!("[v 1:2]"
        nodes: [Call!("v", Args![])],
        errors: [S(3..4, "name must be identifier")]);
}

#[test]
fn test_parse_arrays() {
    // Empty array.
    t!("{()}" Block(Array![]));

    // Array with one item and trailing comma + spans.
    t!("{-(1,)}"
        nodes: [S(0..7, Block(Unary(
            S(1..2, Neg),
            S(2..6, Array![S(3..4, Int(1))])
        )))],
        spans: true);

    // Array with three items and trailing comma.
    t!(r#"{("one", 2, #003,)}"# Block(Array![
        Str("one"),
        Int(2),
        Color(RgbaColor::new(0, 0, 0x33, 0xff))
    ]));

    // Unclosed.
    t!("{(}"
        nodes: [Block(Array![])],
        errors: [S(2..2, "expected closing paren")]);

    // Missing comma + invalid token.
    t!("{(1*/2)}"
        nodes: [Block(Array![Int(1), Int(2)])],
        errors: [S(3..5, "expected expression, found end of block comment"),
                 S(3..3, "expected comma")]);

    // Invalid token.
    t!("{(1, 1u 2)}"
        nodes: [Block(Array![Int(1), Int(2)])],
        errors: [S(5..7, "expected expression, found invalid token")]);

    // Coerced to expression with leading comma.
    t!("{(,1)}"
        nodes: [Block(Int(1))],
        errors: [S(2..3, "expected expression, found comma")]);

    // Missing expression after name makes this an array.
    t!("{(a:)}"
        nodes: [Block(Array![])],
        errors: [S(4..4, "expected expression")]);

    // Expected expression, found named pair.
    t!("{(1, b: 2)}"
        nodes: [Block(Array![Int(1)])],
        errors: [S(5..9, "expected expression, found named pair")]);
}

#[test]
fn test_parse_dictionaries() {
    // Empty dictionary.
    t!("{(:)}" Block(Dict![]));

    // Dictionary with two pairs + spans.
    t!("{(one: 1, two: 2)}"
        nodes: [S(0..18, Block(Dict![
            S(2..5, "one") => S(7..8, Int(1)),
            S(10..13, "two") => S(15..16, Int(2)),
        ]))],
        spans: true);

    // Expected named pair, found expression.
    t!("{(a: 1, b)}"
        nodes: [Block(Dict!["a" => Int(1)])],
        errors: [S(8..9, "expected named pair, found expression")]);

    // Dictionary marker followed by more stuff.
    t!("{(:1 b:2, true::)}"
        nodes: [Block(Dict!["b" => Int(2)])],
        errors: [S(3..4, "expected named pair, found expression"),
                 S(4..4, "expected comma"),
                 S(10..14, "name must be identifier"),
                 S(15..16, "expected expression, found colon")]);
}

#[test]
fn test_parse_expressions() {
    // Parentheses.
    t!("{(x)}{(1)}" Block(Id("x")), Block(Int(1)));

    // Unary operations.
    t!("{-1}"  Block(Unary(Neg, Int(1))));
    t!("{--1}" Block(Unary(Neg, Unary(Neg, Int(1)))));

    // Binary operations.
    t!(r#"{"x"+"y"}"# Block(Binary(Str("x"), Add, Str("y"))));
    t!("{1-2}"        Block(Binary(Int(1), Sub, Int(2))));
    t!("{a * b}"      Block(Binary(Id("a"), Mul, Id("b"))));
    t!("{12pt/.4}"    Block(Binary(Length(12.0, LengthUnit::Pt), Div, Float(0.4))));

    // Associativity.
    t!("{1+2+3}" Block(Binary(Binary(Int(1), Add, Int(2)), Add, Int(3))));
    t!("{1/2*3}" Block(Binary(Binary(Int(1), Div, Int(2)), Mul, Int(3))));

    // Precedence.
    t!("{1+2*-3}" Block(Binary(
        Int(1), Add, Binary(Int(2), Mul, Unary(Neg, Int(3))),
    )));

    // Confusion with floating-point literal.
    t!("{1e-3-4e+4}" Block(Binary(Float(1e-3), Sub, Float(4e+4))));

    // Spans + parentheses winning over precedence.
    t!("{(1+2)*3}"
        nodes: [S(0..9, Block(Binary(
            S(1..6, Binary(S(2..3, Int(1)), S(3..4, Add), S(4..5, Int(2)))),
            S(6..7, Mul),
            S(7..8, Int(3)),
        )))],
        spans: true);

    // Errors.
    t!("{-}{1+}{2*}"
        nodes: [Block(Int(1)), Block(Int(2))],
        errors: [S(2..2, "expected expression"),
                 S(6..6, "expected expression"),
                 S(10..10, "expected expression")]);
}

#[test]
fn test_parse_values() {
    // Basics.
    t!("{_}"      Block(Id("_")));
    t!("{name}"   Block(Id("name")));
    t!("{ke-bab}" Block(Id("ke-bab")));
    t!("{α}"      Block(Id("α")));
    t!("{none}"   Block(Expr::None));
    t!("{true}"   Block(Bool(true)));
    t!("{false}"  Block(Bool(false)));
    t!("{1.0e-4}" Block(Float(1e-4)));
    t!("{3.15}"   Block(Float(3.15)));
    t!("{50%}"    Block(Percent(50.0)));
    t!("{4.5cm}"  Block(Length(4.5, LengthUnit::Cm)));
    t!("{12e1pt}" Block(Length(12e1, LengthUnit::Pt)));

    // Strings.
    t!(r#"{"hi"}"#                     Block(Str("hi")));
    t!(r#"{"a\n[]\"\u{1F680}string"}"# Block(Str("a\n[]\"🚀string")));

    // Colors.
    t!("{#f7a20500}" Block(Color(RgbaColor::new(0xf7, 0xa2, 0x05, 0))));
    t!("{#a5}"
        nodes: [Block(Color(RgbaColor::new(0, 0, 0, 0xff)))],
        errors: [S(1..4, "invalid color")]);

    // Content.
    t!("{{*Hi*}}" Block(Content![Strong, Text("Hi"), Strong]));

    // Invalid tokens.
    t!("{1u}"
        nodes: [],
        errors: [S(1..3, "expected expression, found invalid token")]);
}
