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

/// A lambda-target `before_action` — `before_action -> { … }, only:
/// […]`, no Symbol target and no attached block either — going from
/// "controller class-body macro not recognized" to clean is a claim
/// the emitted dispatcher actually RUNS the lambda's body and applies
/// its `only:` scoping (invariant 6), not just that `check` stops
/// complaining. 233 Procore controllers write exactly this shape for
/// a policy guard closing over a receiverless call
/// (`ensure_permission(documents_policy.configure_tab?)`), not a
/// literal condition inline in the lambda — so this pins that exact
/// pattern: the lambda calls a private predicate method, and the
/// existing "should show article" test is updated to expect the
/// redirect the (permanently failing, for the test) predicate causes.
///
/// NOTE ON SCOPE: a lambda body that reads `params[...]` directly
/// (rather than through a named method) is a NARROWER, separate gap —
/// `rewrite_params`, the pass that turns `params[:x]` into the
/// runtime's string-keyed `@params.fetch("x", ...)`, runs over each
/// action/helper body individually and is never reached by a
/// `PreambleStmt::Block`'s body, which is assembled straight into
/// `process_action` after that pass has already run. None of the 233
/// real sites hit it (`ensure_permission`, `policy.can_view_recycle_bin?`,
/// … all delegate to a named method, whose OWN body goes through the
/// normal per-method pipeline and gets `params` rewritten there); a
/// site that inlined `params[...]` directly in the lambda would not.
/// Recorded here rather than silently left for the next person to
/// rediscover.
#[test]
fn a_lambda_target_before_action_gates_the_action_it_guards() {
    emit_and_run::real_blog()
        .edit(
            "app/controllers/articles_controller.rb",
            "before_action :set_article, only: %i[ show edit update destroy ]",
            "before_action :set_article, only: %i[ show edit update destroy ]\n  before_action -> { redirect_to root_path unless allowed_to_view? }, only: [:show]",
        )
        .edit(
            "app/controllers/articles_controller.rb",
            "  private\n",
            "  private\n\n  def allowed_to_view?\n    false\n  end\n",
        )
        .edit(
            "test/controllers/articles_controller_test.rb",
            "test \"should show article\" do\n    get article_url(@article)\n    assert_response :success\n    assert_select \"h1\", @article.title\n    assert_select \"h2\", \"Comments\"\n    assert_select \"#comments .p-4\", minimum: 1\n  end",
            "test \"a lambda-target before_action redirects when its guard fails\" do\n    get article_url(@article)\n    assert_redirected_to root_url\n  end",
        )
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
