//! Regression: import aliases carry their span through the parser.

use glyph_core::ast::{Decl, ImportKind};
use glyph_core::parse::parse;

#[test]
fn selective_alias_carries_span() {
    let src = "import \"x.glyph\" { foo as bar }\n";
    let (file, _) = parse(src, 0).expect("parse succeeds");
    let imp = match &file.decls[0] {
        Decl::Import(s) => &s.node,
        _ => panic!("expected import decl"),
    };
    let names = match &imp.kind {
        ImportKind::Selective(n) => n,
        _ => panic!("expected selective import"),
    };
    let alias = names[0].alias.as_ref().expect("alias present");
    assert_eq!(alias.node, "bar");
    // `bar` starts at byte 26 in the source above.
    assert_eq!(alias.span.start, 26);
    assert_eq!(alias.span.end, 29);
}

#[test]
fn whole_module_alias_carries_span() {
    let src = "import \"x.glyph\" as mymod\n";
    let (file, _) = parse(src, 0).expect("parse succeeds");
    let imp = match &file.decls[0] {
        Decl::Import(s) => &s.node,
        _ => panic!("expected import decl"),
    };
    let alias = match &imp.kind {
        ImportKind::WholeModule { alias } => alias,
        _ => panic!("expected whole-module import"),
    };
    assert_eq!(alias.node, "mymod");
    assert_eq!(alias.span.start, 20);
    assert_eq!(alias.span.end, 25);
}
