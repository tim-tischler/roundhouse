//! `Array.wrap(x)` — ActiveSupport's class-side `Array` reopen — types
//! without a dispatch failure and lowers to the ported
//! `ActiveSupport.wrap` (`runtime/ruby/active_support_ext.rb`) rather
//! than emitting a literal `Array.wrap` call no transpiled target can
//! serve. 189 occurrences on the Procore corpus (`no known method
//! \`wrap\` on Array`).

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::analyze::{diagnose, Analyzer};
use roundhouse::ingest::ingest_app_from_tree;
use roundhouse::ty::Ty;

const FILES: &[(&str, &str)] = &[
    (
        "db/schema.rb",
        "ActiveRecord::Schema.define do\n  create_table \"users\", force: :cascade do |t|\n    t.string \"tag\"\n  end\nend\n",
    ),
    (
        "app/models/user.rb",
        "class User < ApplicationRecord\n  def tags\n    Array.wrap(tag)\n  end\nend\n",
    ),
];

fn analyzed_app() -> roundhouse::App {
    let tree: HashMap<PathBuf, Vec<u8>> =
        FILES.iter().map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec())).collect();
    let mut app = ingest_app_from_tree(tree).expect("ingest");
    Analyzer::new(&app).analyze(&mut app);
    app
}

#[test]
fn array_wrap_dispatches_and_types_as_an_array() {
    let app = analyzed_app();
    let failures: Vec<_> = diagnose(&app)
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .map(|d| format!("{d:?}"))
        .collect();
    assert!(failures.is_empty(), "unexpected dispatch failures: {failures:?}");

    let model = app.models.iter().find(|m| m.name.0.as_str() == "User").expect("User model");
    let method = model.methods().find(|m| m.name.as_str() == "tags").expect("tags");
    // `tag` is a nilable String column; `Array.wrap` on a non-Array,
    // non-nil argument wraps it in a one-element Array of that type —
    // `Array[String?]` here, not the unioned-with-empty-array shape a
    // naive nil-check would produce (the empty-Array leg of `wrap`'s
    // own definition already covers nil).
    assert_eq!(
        method.body.ty,
        Some(Ty::Array { elem: Box::new(Ty::Union { variants: vec![Ty::Str, Ty::Nil] }) }),
        "{:?}",
        method.body
    );
}

#[test]
fn array_wrap_lowers_to_the_ported_active_support_helper() {
    let mut app = analyzed_app();
    roundhouse::session::analyze_and_lower(&mut app);
    let src = roundhouse::emit::ruby::emit_lowered_models(&app)
        .iter()
        .find(|f| f.path.ends_with("user.rb"))
        .expect("no user.rb emitted")
        .content
        .clone();
    assert!(
        src.contains("ActiveSupport.wrap(tag)"),
        "expected the call rewritten through ActiveSupport.wrap:\n{src}"
    );
    assert!(!src.contains("Array.wrap"), "{src}");
}
