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
