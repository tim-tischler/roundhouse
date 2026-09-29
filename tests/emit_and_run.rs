//! Constructs that `check` accepts must run once emitted.
//!
//! See `tests/support/emit_and_run.rs` for the harness and why it
//! exists. The ignored tests below are known places where the two
//! disagree: `check` is clean and the emitted program fails. Each is a
//! complete statement of the fix: make it pass and drop the `#[ignore]`.

#[path = "support/emit_and_run.rs"]
mod emit_and_run;

/// The harness itself: the unedited blog emits and its controller
/// suite, which renders every page, passes.
#[test]
fn the_unedited_blog_runs() {
    emit_and_run::real_blog()
        .run_test("test/controllers/articles_controller_test.rb")
        .assert_passes();
}

/// #139 typed `Model.human_attribute_name` as a String, which took the
/// call from an error to clean, but no runtime defines it, so every
/// page rendering the form raises `undefined method
/// 'human_attribute_name' for class Article`. It belongs once, in
/// `runtime/ruby/active_record/base.rb`, where every target gets it.
#[test]
#[ignore = "check is clean but the emitted view raises NoMethodError: no runtime defines human_attribute_name (#147)"]
fn human_attribute_name_runs() {
    emit_and_run::real_blog()
        .edit(
            "app/views/articles/_form.html.erb",
            "<%= form.label :title %>",
            "<%= form.label :title %><%= Article.human_attribute_name(:title) %>",
        )
        .run_test("test/controllers/articles_controller_test.rb")
        .assert_passes();
}

/// #140 bound `form_with builder: X`'s block param to `X`, which took a
/// custom builder's own helpers from errors to clean. But the emitted
/// tree cannot load `X` (no runtime `ActionView::Helpers::FormBuilder`
/// to subclass), and the view calls `form.marker_field` on a `form`
/// that no longer exists, because lowering expands the stock builder
/// inline. Passing needs a builder the emitted view can call; until
/// then, the honest state is an error in `check`.
#[test]
#[ignore = "check is clean but the emitted tree fails to load: no runtime FormBuilder, and the inlined form has no builder object (#148)"]
fn a_custom_form_builder_runs() {
    emit_and_run::real_blog()
        .write(
            "app/helpers/custom_form_builder.rb",
            "class CustomFormBuilder < ActionView::Helpers::FormBuilder\n  \
               def marker_field(name)\n    \
                 @template.content_tag(:span, name.to_s, class: \"builder-marker\")\n  \
               end\n\
             end\n",
        )
        .edit(
            "app/views/articles/_form.html.erb",
            "form_with(model: article, class: \"contents\")",
            "form_with(model: article, class: \"contents\", builder: CustomFormBuilder)",
        )
        .edit(
            "app/views/articles/_form.html.erb",
            "<%= form.label :title %>",
            "<%= form.label :title %><%= form.marker_field :title %>",
        )
        .run_test("test/controllers/articles_controller_test.rb")
        .assert_passes();
}

/// `case/in` structural pattern matching (#f9): taking `CaseMatchNode`
/// from an ingest error to a typed `CaseMatch` node is a claim the
/// emitted program actually dispatches through it (invariant 6), not
/// just that `check` stops reporting `unsupported expression node:
/// CaseMatchNode`. A PORO under `app/lib` (same placement as the
/// `Deprecation` overlay above) exercises a `Capture` pattern
/// (`Integer => n`) and a plain-class `Value` pattern (`String`) — the
/// two shapes real-blog's own model/controller code never uses, so
/// this is the only thing that runs them through CRuby at all.
#[test]
fn case_in_pattern_matching_runs() {
    emit_and_run::real_blog()
        .write(
            "app/lib/pattern_matcher.rb",
            "class PatternMatcher\n  \
               def self.classify(x)\n    \
                 case x\n    \
                 in Integer => n\n      \
                   n * 2\n    \
                 in String\n      \
                   0\n    \
                 end\n  \
               end\nend\n",
        )
        .write(
            "test/models/pattern_matcher_test.rb",
            "require \"test_helper\"\n\n\
             class PatternMatcherTest < ActiveSupport::TestCase\n  \
               test \"case/in dispatches by pattern and captures a binding\" do\n    \
                 assert_equal 10, PatternMatcher.classify(5)\n    \
                 assert_equal 0, PatternMatcher.classify(\"hi\")\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/pattern_matcher_test.rb")
        .assert_passes();
}
