//! A dispatch failure on a gem's surface is attributed generically
//! when `gem_owning_constant`'s name-derivation can't match it to any
//! gem in the lockfile, but the constant ALSO isn't defined anywhere
//! in the app itself.
//!
//! `namespace_of` assumes a gem's top-level constant matches its
//! (camelized) gem name — true often enough for `redcarpet` →
//! `Redcarpet`, with a hand-written `IRREGULAR` table for the
//! exceptions it knows about (`bcrypt` → `BCrypt`, `nokogiri` →
//! `Nokogiri`, …). Procore's `procore-instrumentation` gem is not in
//! that table and does not follow the convention at all: it ships a
//! top-level `Observability` module, not `ProcoreInstrumentation` or
//! `Procore`. Every call through `Observability::Tracing.tracer` (61
//! occurrences in the corpus) read as a plain, unattributed
//! `send_dispatch_failed` — accusing the app of a bug that is really
//! an unmodeled gem.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::analyze::attribution::attribute_unknown_gems;
use roundhouse::analyze::{diagnose, Analyzer};
use roundhouse::ingest::ingest_app_from_tree;

const GEMFILE_LOCK: &str = "GEM\n  remote: https://rubygems.org/\n  specs:\n    procore-instrumentation (1.1.4)\n\nPLATFORMS\n  arm64-darwin-24\n\nDEPENDENCIES\n  procore-instrumentation\n\nBUNDLED WITH\n   2.5.9\n";

fn tree(extra: &[(&str, &str)]) -> HashMap<PathBuf, Vec<u8>> {
    let mut files: Vec<(&str, &str)> = vec![
        (
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"users\", force: :cascade do |t|\n  end\nend\n",
        ),
        ("Gemfile.lock", GEMFILE_LOCK),
    ];
    files.extend_from_slice(extra);
    files.iter().map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec())).collect()
}

#[test]
fn a_constant_no_gem_name_matches_and_no_app_class_defines_is_attributed_generically() {
    let mut app = ingest_app_from_tree(tree(&[(
        "app/models/user.rb",
        "class User < ApplicationRecord\n  def trace\n    Observability::Tracing.tracer\n  end\nend\n",
    )]))
    .expect("ingest");
    Analyzer::new(&app).analyze(&mut app);
    let mut diags = diagnose(&app);
    attribute_unknown_gems(&mut diags, &app);

    let failures: Vec<_> = diags
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .collect();
    assert_eq!(failures.len(), 1, "{diags:?}");
    assert_eq!(failures[0].severity, roundhouse::diagnostic::Severity::Info, "{:?}", failures[0]);
    assert!(
        failures[0]
            .message
            .contains("no known constant Observability in the app — probably from an unmodeled gem"),
        "{:?}",
        failures[0]
    );
}

/// The generic fallback must NOT fire when the app itself defines the
/// constant — that is a real, unattributed app bug, not gem coverage,
/// and misattributing it would hide the finding.
#[test]
fn a_constant_the_app_itself_defines_is_not_attributed_as_a_gem_gap() {
    let mut app = ingest_app_from_tree(tree(&[
        (
            "app/models/observability.rb",
            "module Observability\n  def self.whatever\n    1\n  end\nend\n",
        ),
        (
            "app/models/user.rb",
            "class User < ApplicationRecord\n  def trace\n    Observability.nonexistent_method\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    Analyzer::new(&app).analyze(&mut app);
    let mut diags = diagnose(&app);
    attribute_unknown_gems(&mut diags, &app);

    let failures: Vec<_> = diags
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .collect();
    assert_eq!(failures.len(), 1, "{diags:?}");
    assert_eq!(
        failures[0].severity,
        roundhouse::diagnostic::Severity::Error,
        "a real app-defined constant must keep its severity: {:?}",
        failures[0]
    );
}
