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

/// A delegated setter going from broken (`def behavior=\n  x.behavior=\n
/// end` — a `def` with no parameter and a bare `x.y=` call, two syntax
/// errors) to working is a claim the emitted program actually runs a
/// SET through it, not just that `check` stays clean (invariant 6). A
/// PORO under `app/lib` (the same shape `procore_os/deprecation.rb`
/// declares: `attr_accessor` on the target, `delegate` for a setter)
/// forwards to another object — reusing `Article`, since it already
/// has a `title` column — and a model test sets through the forwarder
/// and reads the value back off the target.
#[test]
fn a_delegated_setter_forwards_through_to_its_target() {
    emit_and_run::real_blog()
        .write(
            "app/lib/deprecation.rb",
            "class Deprecation\n  attr_accessor :inner\n\n  delegate :title=, to: :inner\nend\n",
        )
        .write(
            "test/models/deprecation_test.rb",
            "require \"test_helper\"\n\n\
             class DeprecationTest < ActiveSupport::TestCase\n  \
               test \"a delegated setter forwards through to its target\" do\n    \
                 d = Deprecation.new\n    \
                 d.inner = Article.new\n    \
                 d.title = \"Reused\"\n    \
                 assert_equal \"Reused\", d.inner.title\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/deprecation_test.rb")
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

/// `def self.included(klass); class << klass; … end; end` — Procore's
/// shared search concerns (`app/concerns/search_engine/indexed.rb` and
/// more) skip `ActiveSupport::Concern` and open the includer's
/// singleton directly from the vanilla `Module#included` hook. Pins
/// that the resulting class method is actually callable on the
/// including model, not just that `check` no longer reports the
/// `SingletonClassNode` it used to.
#[test]
fn included_hook_class_methods_run() {
    emit_and_run::real_blog()
        .write(
            "app/models/concerns/sluggable.rb",
            "module Sluggable\n  \
               def self.included(klass)\n    \
                 class << klass\n      \
                   def slug_prefix\n        \
                     \"article\"\n      \
                   end\n    \
                 end\n  \
               end\n\
             end\n",
        )
        .edit(
            "app/models/article.rb",
            "class Article < ApplicationRecord\n  has_many :comments, dependent: :destroy",
            "class Article < ApplicationRecord\n  include Sluggable\n\n  \
             has_many :comments, dependent: :destroy",
        )
        .write(
            "test/models/article_sluggable_test.rb",
            "require \"test_helper\"\n\n\
             class ArticleSluggableTest < ActiveSupport::TestCase\n  \
               test \"an included-hook class method is callable on the includer\" do\n    \
                 assert_equal \"article\", Article.slug_prefix\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/article_sluggable_test.rb")
        .assert_passes();
}

/// `scope :x, (lambda do |v| … end)` — Procore's `reports/app/models/
/// report.rb` wraps the spelled-out `lambda`/`proc` scope body in its
/// own parens (`for_tools`, `for_data_sets`, `shared`). Before the fix
/// the parens sat between `parse_scope` and the call it expected, so
/// `check` reported "scope body must be a lambda" — this pins that the
/// emitted scope actually filters, not just that `check` goes quiet.
#[test]
fn parenthesized_lambda_scope_runs() {
    emit_and_run::real_blog()
        .edit(
            "app/models/article.rb",
            "validates :body, presence: true, length: { minimum: 10 }",
            "validates :body, presence: true, length: { minimum: 10 }\n\n  \
             scope :with_title, (lambda do |value|\n    \
               where(title: value)\n  \
             end)",
        )
        .write(
            "test/models/article_scope_test.rb",
            "require \"test_helper\"\n\n\
             class ArticleScopeTest < ActiveSupport::TestCase\n  \
               test \"a parenthesized lambda scope filters by title\" do\n    \
                 found = Article.with_title(\"Getting Started with Rails\").first\n    \
                 assert_not_nil found\n    \
                 assert_equal \"Getting Started with Rails\", found.title\n    \
                 assert_nil Article.with_title(\"No Such Title\").first\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/article_scope_test.rb")
        .assert_passes();
}

/// `enum :x, CONST.map { |v| [v, v.to_s] }.to_h` — Procore's
/// `bid_package.rb` computes an identity string mapping over a
/// constant instead of writing the hash literal out. Pins that the
/// generated predicate and bang-writer methods actually work against a
/// real column, not just that `check` accepts the declaration.
#[test]
fn computed_enum_map_to_h_runs() {
    emit_and_run::real_blog()
        .edit(
            "db/schema.rb",
            "t.string \"title\"\n    t.text \"body\"",
            "t.string \"title\"\n    t.text \"body\"\n    t.string \"kind\", default: \"post\", null: false",
        )
        .edit(
            "app/models/article.rb",
            "class Article < ApplicationRecord\n  has_many :comments, dependent: :destroy",
            "class Article < ApplicationRecord\n  has_many :comments, dependent: :destroy\n\n  \
             KINDS = %i[post announcement]\n  \
             enum :kind, KINDS.map { |k| [k, k.to_s] }.to_h",
        )
        .write(
            "test/models/article_enum_test.rb",
            "require \"test_helper\"\n\n\
             class ArticleEnumTest < ActiveSupport::TestCase\n  \
               test \"a computed .map{}.to_h enum mapping generates working predicates\" do\n    \
                 article = articles(:one)\n    \
                 assert article.post?\n    \
                 article.announcement!\n    \
                 assert article.announcement?\n    \
                 assert_equal \"announcement\", article.kind\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/article_enum_test.rb")
        .assert_passes();
}

/// Gap F15 took `&method(:name)` from an ingest error (block-argument
/// forms other than `&:symbol`/`&local_var` were unsupported) to
/// clean, emitting `&method(:name)` verbatim (`ExprNode::MethodRef`).
/// Invariant 6: prove the emitted PROGRAM runs it, not just that
/// `check` stays quiet — a PORO under `app/lib` maps an array through
/// a bound-method reference to its own helper, and a model test reads
/// the result back.
#[test]
fn method_ref_block_arg_runs() {
    emit_and_run::real_blog()
        .write(
            "app/lib/doubler.rb",
            "class Doubler\n  \
               def self.doubled(list)\n    \
                 list.map(&method(:double))\n  \
               end\n\n  \
               def self.double(n)\n    \
                 n * 2\n  \
               end\n\
             end\n",
        )
        .write(
            "test/models/doubler_test.rb",
            "require \"test_helper\"\n\n\
             class DoublerTest < ActiveSupport::TestCase\n  \
               test \"&method(:name) as a block argument runs\" do\n    \
                 assert_equal [2, 4, 6], Doubler.doubled([1, 2, 3])\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/doubler_test.rb")
        .assert_passes();
}

/// Gap F7 took `def x(...)` / `y(...)` from an ingest error
/// (`ForwardingParameterNode`/`ForwardingArgumentsNode` had no arm)
/// to clean, desugaring to the `*args, **kwargs, &blk` trio the IR
/// already models. Invariant 6: prove the emitted program actually
/// forwards all three — positional args, a keyword arg, AND a block —
/// through one `...` call, not just that `check` stays quiet.
#[test]
fn forwarding_call_forwards_args_kwargs_and_block() {
    // Two shapes to steer clear of, both pre-existing kwsplat/ingest
    // approximations this task doesn't own:
    //   - An explicit LOCAL-VAR receiver, not an ivar: `lower::
    //     kwsplat`'s `callee_params` matches `Ty::Class` exactly and
    //     doesn't peel a `Ty::Union` — an ivar's inferred type is
    //     nilable (`Combiner | Nil`), which fails that match, so the
    //     `**` re-expansion silently declines for an ivar receiver.
    //   - A REQUIRED keyword (`factor:`), not an optional one with a
    //     default: `ingest_library_method` flattens a LONE optional
    //     keyword to a plain positional-with-default (`factor = 1`)
    //     when nothing else in the signature forces true keyword
    //     representation, and `lower::kwsplat`'s erased-splat
    //     recognition only fires for a param whose `Param.keyword` is
    //     still `true` post-flattening.
    emit_and_run::real_blog()
        .write(
            "app/lib/multiplier.rb",
            "class Multiplier\n  \
               def call(...)\n    \
                 helper = Combiner.new\n    \
                 helper.combine(...)\n  \
               end\n\
             end\n\n\
             class Combiner\n  \
               def combine(a, b, factor:, &blk)\n    \
                 result = (a + b) * factor\n    \
                 blk ? blk.call(result) : result\n  \
               end\n\
             end\n",
        )
        .write(
            "test/models/multiplier_test.rb",
            "require \"test_helper\"\n\n\
             class MultiplierTest < ActiveSupport::TestCase\n  \
               test \"...forwards positional args, kwargs, and a block\" do\n    \
                 assert_equal 11, Multiplier.new.call(2, 3, factor: 2) { |r| r + 1 }\n  \
               end\n\
             end\n",
        )
        .run_test("test/models/multiplier_test.rb")
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
