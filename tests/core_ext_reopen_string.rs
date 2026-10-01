//! An app-level reopen of a Ruby core class (`class String; def
//! to_bool; …; end; end` under `lib/core_ext/`, Procore's own shape)
//! types on dispatch, not just on ingest.
//!
//! `lib/` is walked as an ordinary support root (`ingest::app`), so
//! `class String ... end` ingests as a ordinary `LibraryClass` keyed
//! `ClassId("String")`, and the general return-harvest types its
//! methods the same as any other class's. What was missing: dispatch
//! on a String-typed VALUE (`"foo".to_bool`) never consulted that
//! registry entry — `str_method`'s hardcoded Ruby-core-plus-
//! ActiveSupport table is "the authority" by its own doc comment, and
//! a miss there answered `unknown()` outright. Every call to a
//! reopened method read as `send_dispatch_failed`, however many call
//! sites used it — 265 occurrences of `to_bool` alone on the Procore
//! corpus (206 on `String?`, 59 on `String`).

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::analyze::{diagnose, Analyzer};
use roundhouse::ingest::ingest_app_from_tree;
use roundhouse::ty::Ty;

const FILES: &[(&str, &str)] = &[
    (
        "db/schema.rb",
        "ActiveRecord::Schema.define do\n  create_table \"users\", force: :cascade do |t|\n    t.string \"flag\", null: false\n  end\nend\n",
    ),
    (
        "lib/core_ext/string.rb",
        "class String\n  def to_bool\n    self == \"true\"\n  end\nend\n",
    ),
    (
        "app/models/user.rb",
        "class User < ApplicationRecord\n  def enabled?\n    flag.to_bool\n  end\n\n  def shout\n    flag.upcase\n  end\nend\n",
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
fn a_reopened_core_class_method_dispatches_without_error() {
    let app = analyzed_app();
    let failures: Vec<_> = diagnose(&app)
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .map(|d| format!("{d:?}"))
        .collect();
    assert!(failures.is_empty(), "unexpected dispatch failures: {failures:?}");

    // `flag.to_bool`'s body is `self == "true"` — a `Bool` return, per
    // the `==` entry every other String method shares. Checking the
    // concrete type (not just the absence of an error) catches a
    // fallback that answers `Untyped`/gradual instead of the real
    // registered return type.
    let model = app.models.iter().find(|m| m.name.0.as_str() == "User").expect("User model");
    let method = model.methods().find(|m| m.name.as_str() == "enabled?").expect("enabled?");
    assert_eq!(method.body.ty, Some(Ty::Bool), "{:?}", method.body);
}

/// A method the reopen does NOT define (`upcase`) still dispatches
/// through the real Ruby-core table, typed as the table says (`Str`)
/// — the registry consult is a fallback on a table MISS, not a
/// replacement for the table.
#[test]
fn the_tables_core_methods_still_win_over_a_registry_miss() {
    let app = analyzed_app();
    let model = app.models.iter().find(|m| m.name.0.as_str() == "User").expect("User model");
    let method = model.methods().find(|m| m.name.as_str() == "shout").expect("shout");
    assert_eq!(method.body.ty, Some(Ty::Str), "{:?}", method.body);
}
