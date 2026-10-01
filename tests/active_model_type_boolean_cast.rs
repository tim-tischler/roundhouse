//! `ActiveModel::Type::Boolean.new.cast(x)` — ActiveModel's own
//! truthy-string coercion (`"false"`/`"0"`/`false`/`0` → `false`,
//! everything else → `true`) — types as `Bool` rather than failing
//! dispatch. `ActiveModel::Type::Boolean` is a framework class never
//! ingested from app source, so the class registry never has an entry
//! for it; every call through `.new.cast(...)` fell through to
//! `unknown()`. 84 occurrences on the Procore corpus.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::analyze::{diagnose, Analyzer};
use roundhouse::ingest::ingest_app_from_tree;
use roundhouse::ty::Ty;

const FILES: &[(&str, &str)] = &[
    (
        "db/schema.rb",
        "ActiveRecord::Schema.define do\n  create_table \"users\", force: :cascade do |t|\n    t.string \"flag\"\n  end\nend\n",
    ),
    (
        "app/models/user.rb",
        "class User < ApplicationRecord\n  def enabled?\n    ActiveModel::Type::Boolean.new.cast(flag)\n  end\nend\n",
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
fn active_model_type_boolean_cast_dispatches_and_types_as_bool() {
    let app = analyzed_app();
    let failures: Vec<_> = diagnose(&app)
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .map(|d| format!("{d:?}"))
        .collect();
    assert!(failures.is_empty(), "unexpected dispatch failures: {failures:?}");

    let model = app.models.iter().find(|m| m.name.0.as_str() == "User").expect("User model");
    let method = model.methods().find(|m| m.name.as_str() == "enabled?").expect("enabled?");
    assert_eq!(method.body.ty, Some(Ty::Bool), "{:?}", method.body);
}
