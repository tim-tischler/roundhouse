//! `extend(Module.new { … })` — P19t's spelling in
//! `app/lib/p19t.rb` of the same "extend self" idea
//! (`tests/extend_self_module.rs`), one level more anonymous: instead
//! of naming a sibling module, the module literal is built inline as
//! the `extend` call's own argument. Ruby's `extend` makes every
//! instance method of the literal a singleton method of the receiver,
//! so `P19t.f` / `P19t.l` (aliased inside the literal from
//! `formatted_localized` / `localized`) need to resolve on `P19t`
//! itself.
//!
//! Before this shape was recognized, the whole `extend(...)` call fell
//! through to the generic "unrecognized call" capture: `ingest_expr`
//! on the statement treats it as one opaque expression and never
//! descends into the block argument, so every `def` and
//! `alias_method` inside it — P19t's entire public surface — was
//! silently dropped. 778+ `send_dispatch_failed` sites on
//! `no known method \`f\`/\`l\` on P19t` in the Procore corpus traced
//! to this one shape.
//!
//! Also covers the sibling gap `alias_method` exposed: the generic
//! class/module body walk (`library_class.rs::walk_decl_body`) had no
//! handling for `alias_method` at all outside a model's
//! `class << self` block (`model.rs::ingest_singleton_class_methods`)
//! — any library class aliasing a method directly in its own body hit
//! the same silent drop.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::emit::ruby;
use roundhouse::ingest::ingest_app_from_tree;

fn tree(files: &[(&str, &str)]) -> HashMap<PathBuf, Vec<u8>> {
    files
        .iter()
        .map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec()))
        .collect()
}

fn emitted(lib_src: &str) -> String {
    let mut app = ingest_app_from_tree(tree(&[
        (
            "db/schema.rb",
            "ActiveRecord::Schema.define do\n  create_table \"posts\", force: :cascade do |t|\n    t.string \"body\", null: false\n  end\nend\n",
        ),
        ("app/models/post.rb", "class Post < ApplicationRecord\nend\n"),
        ("app/models/p19t.rb", lib_src),
    ]))
    .expect("ingest");
    roundhouse::session::analyze_and_lower(&mut app);
    ruby::emit_library(&app)
        .iter()
        .find(|f| f.path.ends_with("p19t.rb"))
        .expect("no p19t.rb emitted")
        .content
        .clone()
}

#[test]
fn extend_of_an_inline_module_new_makes_every_def_callable_on_the_module() {
    let src = emitted(
        r#"module P19t
  extend(Module.new {
    def formatted_localized(date_or_time, object_with_location, options={})
      date_or_time
    end
    alias_method :f, :formatted_localized

    def localized(date_or_time, object_with_location)
      date_or_time
    end
    alias_method :l, :localized
  })
end
"#,
    );
    assert!(src.contains("def self.formatted_localized"), "{src}");
    assert!(src.contains("def self.localized"), "{src}");
    // The aliases are the whole point: P19t.f / P19t.l are the names
    // every call site in the corpus actually dispatches.
    assert!(src.contains("def self.f("), "{src}");
    assert!(src.contains("def self.l("), "{src}");
}

/// `alias_method` directly in a library class/module body (no
/// `extend(Module.new {})` literal involved) resolves the same way —
/// the generic fix, not just the P19t-shaped special case. The target
/// and the alias share the module's own top-level body, same as
/// P19t's literal does internally; `alias_method` resolving against a
/// target defined in an OUTER scope (e.g. a sibling `class << self`
/// block) is not covered by this fix.
#[test]
fn alias_method_in_a_plain_module_body_resolves_to_the_target() {
    let src = emitted(
        r#"module P19t
  def self.formatted_localized(date_or_time, object_with_location, options={})
    date_or_time
  end
  alias_method :f, :formatted_localized
end
"#,
    );
    assert!(src.contains("def self.formatted_localized"), "{src}");
    assert!(src.contains("def self.f("), "{src}");
}

/// `extend(Module.new { … })` with NO `def`s inside doesn't crash
/// ingest — an empty literal is a no-op, not a panic. (Uses the
/// plural ingest directly: a module with no methods, constants, or
/// `included do` block isn't worth surfacing as a `LibraryClass` at
/// all per `module_has_direct_def`, so there is no emitted file to
/// inspect — the assertion here is "didn't error", not "emitted
/// something".)
#[test]
fn extend_of_an_empty_module_new_literal_does_not_panic() {
    let out = roundhouse::ingest::library_class::ingest_library_classes(
        b"module P19t\n  extend(Module.new {\n  })\nend\n",
        "p19t.rb",
    )
    .expect("ingest should not error");
    assert!(out.is_empty(), "{out:?}");
}

/// `extend SomeOtherClass.new { … }` (not the bare `Module` constant)
/// must not be mistaken for the P19t shape — `module_new_block` only
/// matches a literal `Module.new` receiver, so this stays an ordinary
/// unrecognized call, captured the same way it always was. The module
/// has no OTHER recognized method, so (like the empty-literal case
/// above) it isn't surfaced as a `LibraryClass` at all — proving the
/// `def` inside didn't leak out as a method of `P19t`.
#[test]
fn extend_of_a_non_module_new_literal_is_not_the_p19t_shape() {
    let out = roundhouse::ingest::library_class::ingest_library_classes(
        b"module P19t\n  extend(Struct.new {\n    def formatted_localized(x)\n      x\n    end\n  })\nend\n",
        "p19t.rb",
    )
    .expect("ingest should not error");
    assert!(out.is_empty(), "{out:?}");
}
