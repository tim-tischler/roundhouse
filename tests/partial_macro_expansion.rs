//! Partial expansion of a concern-exported class-body macro whose body
//! mixes filter DSL with statements that are not filter DSL
//! (`ingest::app::expand_class_body_macros`), plus the three related
//! shapes it now recognizes:
//!
//!   1. a macro body that mixes a class-level config write with real
//!      filter DSL — Procore's `permit_with` is exactly this shape
//!      (`_permissions.permit_with(policy_class)` then
//!      `around_action(:enforce_action_policy, unless: …)`). The OLD
//!      all-or-nothing policy dropped the `around_action` entirely
//!      because ONE statement wasn't filter DSL; this expands the
//!      filter half regardless and ledgers the config write quietly.
//!   2. a concern macro called with a block it stores for later
//!      (`authorize(:create) { loader }`) — typed as controller-
//!      instance code, never added to the filter chain.
//!   3. a macro-of-macros: a plain `def self.foo` defined directly on
//!      a base controller, calling other class-body macros in turn.
//!   4. a macro name defined nowhere this walk ingested a body for —
//!      attributed to an unmodeled gem when the app's `Gemfile.lock`
//!      names one, or left as an honest "not defined in the app"
//!      otherwise — kept distinct from the original "not recognized"
//!      bucket, which stays reserved for a name the walk DID find a
//!      body for somewhere.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::App;
use roundhouse::dialect::{ControllerBodyItem, FilterKind};
use roundhouse::ingest::ingest_app_from_tree;

fn tree(files: &[(&str, &str)]) -> HashMap<PathBuf, Vec<u8>> {
    files.iter().map(|(path, src)| (PathBuf::from(*path), src.as_bytes().to_vec())).collect()
}

fn controller<'a>(app: &'a App, name: &str) -> &'a roundhouse::dialect::Controller {
    app.controllers.iter().find(|c| c.name.0.as_str() == name).unwrap_or_else(|| {
        panic!(
            "{name} not ingested; have: {:?}",
            app.controllers.iter().map(|c| c.name.0.as_str()).collect::<Vec<_>>()
        )
    })
}

fn filters(c: &roundhouse::dialect::Controller) -> Vec<&roundhouse::dialect::Filter> {
    c.body
        .iter()
        .filter_map(|item| match item {
            ControllerBodyItem::Filter { filter, .. } => Some(filter),
            _ => None,
        })
        .collect()
}

fn action_names(c: &roundhouse::dialect::Controller) -> Vec<String> {
    c.actions().map(|a| a.name.as_str().to_string()).collect()
}

fn survey_messages(tree_files: HashMap<PathBuf, Vec<u8>>) -> Vec<String> {
    roundhouse::ingest::survey::activate();
    let _app = ingest_app_from_tree(tree_files).expect("ingest");
    roundhouse::ingest::survey::drain().iter().map(|g| format!("{g:?}")).collect()
}

// ── 1. Partial expansion: a config write beside real filter DSL ──────

const PERMISSIONS_CONCERN: &str = r#"
module Permissions
  extend ActiveSupport::Concern

  class_methods do
    def permit_with(policy)
      configuration.policy_class = policy
      around_action :enforce, unless: :skip_policy_enforcement?
    end
  end

  private
    def enforce
      yield
    end

    def skip_policy_enforcement?
      false
    end
end
"#;

fn app_controller_with(body: &str) -> App {
    let files = tree(&[
        ("app/controllers/concerns/permissions.rb", PERMISSIONS_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            &format!("class ThingsController < ApplicationController\n{body}\nend\n"),
        ),
    ]);
    ingest_app_from_tree(files).expect("ingest")
}

#[test]
fn a_config_write_beside_filter_dsl_still_lets_the_filter_through() {
    let app = app_controller_with("  permit_with FooPolicy\n\n  def show\n  end\n");
    let c = controller(&app, "ThingsController");
    let fs = filters(c);
    assert_eq!(fs.len(), 1, "the around_action should still be expanded: {fs:?}");
    assert_eq!(fs[0].kind, FilterKind::Around);
    assert_eq!(fs[0].target.as_str(), "enforce");
    assert_eq!(fs[0].unless_cond.as_ref().map(|s| s.as_str()), Some("skip_policy_enforcement?"));
}

#[test]
fn the_config_write_earns_exactly_one_quiet_ledger_line_and_nothing_else() {
    let messages = survey_messages(tree(&[
        ("app/controllers/concerns/permissions.rb", PERMISSIONS_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  permit_with FooPolicy\n\n  def show\n  end\nend\n",
        ),
    ]));
    let config_lines: Vec<_> =
        messages.iter().filter(|m| m.contains("class-body macro config not modeled")).collect();
    assert_eq!(config_lines.len(), 1, "expected exactly one config note: {messages:?}");
    assert!(
        config_lines[0].contains("permit_with") && config_lines[0].contains("policy_class"),
        "the config line should name the macro and what it stores: {config_lines:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("not expanded") || m.contains("not recognized")),
        "the filter half was fully expanded; nothing here should read as unresolved: {messages:?}"
    );
}

// ── 2. A block-registering macro types as controller code ────────────

const AUTHORIZE_CONCERN: &str = r#"
module Permissions
  extend ActiveSupport::Concern

  class_methods do
    def authorize(*actions, &loader)
    end
  end
end
"#;

#[test]
fn a_block_registering_macro_is_not_placed_in_the_filter_chain() {
    let app = ingest_app_from_tree(tree(&[
        ("app/controllers/concerns/permissions.rb", AUTHORIZE_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  authorize(:create) { some_undefined_loader }\n\n  def create\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    let c = controller(&app, "ThingsController");
    assert!(filters(c).is_empty(), "a stored block is not a filter: {:?}", filters(c));
    let synthesized: Vec<_> =
        action_names(c).into_iter().filter(|n| n.starts_with("__authorize_block_")).collect();
    assert_eq!(synthesized.len(), 1, "expected one synthesized helper: {:?}", action_names(c));
}

#[test]
fn the_block_macro_earns_a_not_sequenced_ledger_line() {
    let messages = survey_messages(tree(&[
        ("app/controllers/concerns/permissions.rb", AUTHORIZE_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  authorize(:create) { some_undefined_loader }\n\n  def create\n  end\nend\n",
        ),
    ]));
    assert!(
        messages.iter().any(|m| m.contains("class-body macro block not sequenced")
            && m.contains("authorize")
            && m.contains("not placed in the filter chain")),
        "expected the block-not-sequenced line: {messages:?}"
    );
}

#[test]
fn the_synthesized_helpers_undefined_call_surfaces_as_a_real_dispatch_failure() {
    let mut app = ingest_app_from_tree(tree(&[
        ("app/controllers/concerns/permissions.rb", AUTHORIZE_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  authorize(:create) { some_undefined_loader }\n\n  def create\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    let _ = roundhouse::session::analyze_and_lower(&mut app);
    let diags = roundhouse::analyze::diagnose(&app);
    // Typed in controller-instance context (proving item 2 actually
    // types the block body, not just files a note about it): a
    // receiverless call to a name nothing defines anywhere has no type
    // to resolve to, exactly like the same call would if it sat
    // directly in an ordinary private method.
    assert!(
        diags.iter().any(|d| d.code() == "unresolved_type"
            && format!("{:?}", d.kind).contains("some_undefined_loader")),
        "expected an unresolved-type diagnostic on the synthesized helper's body: {diags:?}"
    );
}

// ── 3. Macro-of-macros: a `def self.x` calling other macros ──────────

const MOM_PERMISSIONS: &str = r#"
module Permissions
  extend ActiveSupport::Concern

  class_methods do
    def permit_with(policy)
      configuration.policy_class = policy
      around_action :enforce, unless: :skip_policy_enforcement?
    end

    def authorize(*actions, &loader)
    end
  end

  private
    def enforce
      yield
    end

    def skip_policy_enforcement?
      false
    end
end
"#;

const MOM_BASE_CONTROLLER: &str = r#"
class BaseThingsController < ApplicationController
  def self.authorize_all_actions!
    permit_with FooPolicy
    authorize(:create) { some_loader }
  end
end
"#;

#[test]
fn a_macro_of_macros_expands_one_level() {
    let app = ingest_app_from_tree(tree(&[
        ("app/controllers/concerns/permissions.rb", MOM_PERMISSIONS),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        ("app/controllers/base_things_controller.rb", MOM_BASE_CONTROLLER),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < BaseThingsController\n  authorize_all_actions!\n\n  def create\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    let c = controller(&app, "ThingsController");
    let fs = filters(c);
    assert_eq!(fs.len(), 1, "permit_with's around_action should surface through the self-macro: {fs:?}");
    assert_eq!(fs[0].target.as_str(), "enforce");
    let synthesized: Vec<_> =
        action_names(c).into_iter().filter(|n| n.starts_with("__authorize_block_")).collect();
    assert_eq!(
        synthesized.len(),
        1,
        "authorize's block should ALSO surface through the self-macro: {:?}",
        action_names(c)
    );
}

// ── 4. A macro name this walk never sees a body for ───────────────────

const GEMFILE_LOCK_WITH_UNKNOWN_GEM: &str = "GEM\n  remote: https://rubygems.org/\n  specs:\n    acme-sift (1.0.0)\n\nDEPENDENCIES\n  acme-sift\n";

#[test]
fn an_undefined_macro_is_attributed_to_an_unmodeled_gem_when_one_is_locked() {
    let messages = survey_messages(tree(&[
        ("Gemfile.lock", GEMFILE_LOCK_WITH_UNKNOWN_GEM),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  sort_on :name\n\n  def index\n  end\nend\n",
        ),
    ]));
    assert!(
        messages
            .iter()
            .any(|m| m.contains("controller class-body macro from an unmodeled gem") && m.contains("sort_on")),
        "expected the unmodeled-gem line: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("not recognized") || m.contains("not defined in the app")),
        "should not ALSO fall into the other two buckets: {messages:?}"
    );
}

#[test]
fn an_undefined_macro_with_no_gem_lock_data_gets_the_weaker_honest_line() {
    let messages = survey_messages(tree(&[
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  sort_on :name\n\n  def index\n  end\nend\n",
        ),
    ]));
    assert!(
        messages
            .iter()
            .any(|m| m.contains("controller class-body macro not defined in the app") && m.contains("sort_on")),
        "expected the weaker honest line when there is no gem_lock at all: {messages:?}"
    );
}

#[test]
fn a_rails_core_macro_with_an_unsupported_shape_is_never_blamed_on_a_gem() {
    // `protect_from_forgery` is Rails/ActionController's own macro.
    // `parse_forgery_macro` only models the `:exception` strategy, so
    // `with: :null_session` reaches the generic bucket the same way an
    // app-defined macro's unsupported shape would — but Rails is never
    // "ingested" as app source, so it would otherwise ALWAYS look like
    // a name nothing here defines, misfiring the gem heuristic exactly
    // the way it did on procore-slim before `RAILS_CORE_FILTER_MACROS`
    // existed.
    let messages = survey_messages(tree(&[
        ("Gemfile.lock", GEMFILE_LOCK_WITH_UNKNOWN_GEM),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  protect_from_forgery with: :null_session\nend\n",
        ),
    ]));
    assert!(
        messages.iter().any(|m| {
            m.contains("controller class-body macro not recognized") && m.contains("protect_from_forgery")
        }),
        "expected the original generic line, not a gem claim: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("unmodeled gem") || m.contains("not defined in the app")),
        "Rails' own macro must never be attributed to a gem or to \"not defined in the app\": {messages:?}"
    );
}

#[test]
fn a_name_the_walk_did_ingest_a_body_for_keeps_the_original_generic_line() {
    // `sort_on` IS defined somewhere this walk saw — as an ordinary
    // (unrelated) model method — so the gap is "our shape recognizer
    // missed it," not "a gem's DSL." The original bucket is reserved
    // for exactly this case.
    let messages = survey_messages(tree(&[
        ("Gemfile.lock", GEMFILE_LOCK_WITH_UNKNOWN_GEM),
        ("app/models/widget.rb", "class Widget < ApplicationRecord\n  def sort_on(x)\n    x\n  end\nend\n"),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  sort_on :name\n\n  def index\n  end\nend\n",
        ),
    ]));
    assert!(
        messages
            .iter()
            .any(|m| m.contains("controller class-body macro not recognized") && m.contains("sort_on")),
        "expected the original generic line: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("unmodeled gem") || m.contains("not defined in the app")),
        "should not ALSO claim a gem origin once the app itself defines the name: {messages:?}"
    );
}

// ── 5. The rest of the Rails/Ruby core vocabulary exclusion ──────────
// A full Procore run surfaced four more names the gem heuristic
// misfired on beyond `protect_from_forgery`: `class_attribute`,
// `prepend_around_action`, `wrap_parameters`, and `require`. None of
// these are consumed elsewhere (their effect really is dropped, same
// as before this feature existed), so they must still read as the
// original generic "not recognized" line — just never as a gem.

#[test]
fn rails_and_ruby_core_vocabulary_is_never_blamed_on_a_gem() {
    let messages = survey_messages(tree(&[
        ("Gemfile.lock", GEMFILE_LOCK_WITH_UNKNOWN_GEM),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  \
               class_attribute :foo\n  \
               prepend_around_action :bar\n  \
               wrap_parameters :baz\n  \
               require 'set'\nend\n",
        ),
    ]));
    for name in ["class_attribute", "prepend_around_action", "wrap_parameters", "require"] {
        assert!(
            messages
                .iter()
                .any(|m| m.contains("controller class-body macro not recognized") && m.contains(name)),
            "expected `{name}` to keep the original generic line: {messages:?}"
        );
    }
    assert!(
        !messages.iter().any(|m| m.contains("unmodeled gem") || m.contains("not defined in the app")),
        "none of Ruby's or Rails' own vocabulary should ever read as a gem or as \"not defined in the app\": {messages:?}"
    );
}

// ── 6. A splat-of-constant filter target resolves post-ingest ────────
// `skip_before_action(*SomeConst, only: [...])` is ordinary Rails —
// the macro runs with the constant's REAL value at class-definition
// time — but `parse_filter_call` (ingest, one file at a time) can
// never resolve `SomeConst` against another file's constant. This is
// exactly `document_markup/viewer.rb`'s
// `skip_before_action(*DocumentMarkup::Jwt::TRADITIONAL_AUTHENTICATION_METHODS,
// only: [...])` on the real Procore corpus, reproduced here with a
// synthetic constant.

const JWT_CONST_SOURCE: &str = r#"
module DocumentMarkup
  class Jwt
    TRADITIONAL_AUTHENTICATION_METHODS = [:authorize, :verify_authenticity_token, :ensure_accepted_terms_and_conditions].freeze
  end
end
"#;

#[test]
fn a_splat_of_a_resolvable_constant_expands_to_real_filter_targets() {
    let app = ingest_app_from_tree(tree(&[
        ("app/services/document_markup/jwt.rb", JWT_CONST_SOURCE),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  \
               skip_before_action(*DocumentMarkup::Jwt::TRADITIONAL_AUTHENTICATION_METHODS, only: [:jwt_file])\n\n  \
               def jwt_file\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    let c = controller(&app, "ThingsController");
    let fs = filters(c);
    let targets: Vec<String> =
        fs.iter().filter(|f| f.kind == FilterKind::Skip).map(|f| f.target.as_str().to_string()).collect();
    assert_eq!(
        targets,
        vec![
            "authorize".to_string(),
            "verify_authenticity_token".to_string(),
            "ensure_accepted_terms_and_conditions".to_string()
        ],
        "the splat should expand to the constant's three real elements, in order: {fs:?}"
    );
    assert!(
        fs.iter().all(|f| f.only.len() == 1 && f.only[0].as_str() == "jwt_file"),
        "the `only:` scoping must apply to every target the splat expanded to: {fs:?}"
    );
}

#[test]
fn a_splat_of_a_resolvable_constant_inside_a_concerns_included_do_block_also_resolves() {
    // The actual Procore shape: the splat-target `skip_before_action`
    // lives in a CONCERN's `included do` block, not directly in the
    // controller — it must be spliced into the controller's own body
    // before this resolves, with no special-casing needed for the
    // splice origin.
    let app = ingest_app_from_tree(tree(&[
        ("app/services/document_markup/jwt.rb", JWT_CONST_SOURCE),
        (
            "app/controllers/concerns/viewer.rb",
            "module Viewer\n  extend ActiveSupport::Concern\n\n  included do\n    \
               skip_before_action(*DocumentMarkup::Jwt::TRADITIONAL_AUTHENTICATION_METHODS, only: [:jwt_file])\n  \
               end\nend\n",
        ),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  include Viewer\n\n  def jwt_file\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    // The splice lands in the INCLUDING class's own body — exactly
    // `ProjectArea::MergedPdfsController`'s real shape, which includes
    // `ImageProcessingAuthentication::PayloadVerification` directly
    // rather than through an ancestor. A subclass inheriting the
    // concern transitively picks up the filter through the ancestor
    // chain at analyze time, a separate mechanism this test isn't
    // after.
    let c = controller(&app, "ThingsController");
    let fs = filters(c);
    assert_eq!(
        fs.iter().filter(|f| f.kind == FilterKind::Skip).count(),
        3,
        "the spliced-in splat should expand the same way it would directly in the controller: {fs:?}"
    );
}

// ── 7. A class-side `delegate` macro call without a block ────────────
// Procore's real `authorize`/`permit_undeclared_actions!` are not a
// literal `def self.x` — they're `delegate :authorize, :permit_undeclared_actions!,
// to: :_permissions` inside `class_methods do`. `authorize(*actions, &loader)`'s
// REAL definition takes a block, but a BLOCKLESS call
// (`authorize :create, :update`) is ordinary actions-only registration
// into the target's AttributeLoader — a config write, not "not
// recognized".

const DELEGATED_AUTHORIZE_CONCERN: &str = r#"
module Permissions
  extend ActiveSupport::Concern

  class_methods do
    def permit_with(policy_class)
      _permissions.permit_with(policy_class)
      around_action(:enforce_action_policy, unless: :skip_policy_enforcement?)
    end

    delegate :authorize, :permit_undeclared_actions!, to: :_permissions

    def _permissions
      @_permissions ||= Object.new
    end
  end
end
"#;

#[test]
fn a_blockless_delegated_class_macro_classifies_as_config_not_unrecognized() {
    let messages = survey_messages(tree(&[
        ("app/controllers/concerns/permissions.rb", DELEGATED_AUTHORIZE_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  authorize :create, :update\n\n  def create\n  end\nend\n",
        ),
    ]));
    assert!(
        messages.iter().any(|m| m.contains("class-body macro config not modeled")
            && m.contains("authorize")
            && m.contains("_permissions")),
        "expected a config note naming the forwarding target: {messages:?}"
    );
    assert!(
        !messages.iter().any(|m| m.contains("not recognized") && m.contains("authorize")),
        "a blockless delegated macro call should not ALSO earn the generic line: {messages:?}"
    );
}

#[test]
fn a_blockless_delegated_class_macro_is_dropped_not_kept_as_unknown() {
    // The config note already says what the call does; no Unknown item
    // should remain to earn a second, less specific line downstream.
    let app = ingest_app_from_tree(tree(&[
        ("app/controllers/concerns/permissions.rb", DELEGATED_AUTHORIZE_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\n  include Permissions\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  authorize :create, :update\n\n  def create\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    let c = controller(&app, "ThingsController");
    let unknown_count = c
        .body
        .iter()
        .filter(|item| matches!(item, ControllerBodyItem::Unknown { .. }))
        .count();
    assert_eq!(unknown_count, 0, "the delegated macro call should be dropped, not kept Unknown: {:?}", c.body);
}

#[test]
fn a_splat_of_an_unresolvable_constant_still_falls_back_honestly() {
    let messages = survey_messages(tree(&[
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "app/controllers/things_controller.rb",
            "class ThingsController < ApplicationController\n  \
               skip_before_action(*SomeUnknownModule::NOT_A_REAL_CONST, only: [:jwt_file])\n\n  \
               def jwt_file\n  end\nend\n",
        ),
    ]));
    assert!(
        messages.iter().any(|m| {
            m.contains("controller class-body macro not recognized") && m.contains("skip_before_action")
        }),
        "an unresolvable splat must still fall back to the honest generic line, not be silently dropped: {messages:?}"
    );
}

// ── 8. A controller's own ancestor lives under `components/*/app/` ───
// Procore's real shape: `Rest::V2::Internal::Authorization::Context::V0::
// DomainsController < Rest::V2::ApplicationController`, where
// `Rest::V2::ApplicationController` — the controller that actually
// `include`s the permissions concern — lives at `components/api/app/
// controllers/rest/v2/application_controller.rb`, NOT under the
// conventional root `app/controllers/`. Before `app_subdir_roots`,
// that file was never ingested as a `Controller` at all (only files
// directly under the app root's `app/controllers/` were), so the
// ancestor-chain walk `expand_class_body_macros` relies on could never
// find it, and the leaf controller's `permit_with` stayed unrecognized
// no matter how correct everything else was.

#[test]
fn a_component_scoped_ancestor_controller_is_ingested_and_resolves_its_macro() {
    let app = ingest_app_from_tree(tree(&[
        ("app/controllers/concerns/permissions.rb", PERMISSIONS_CONCERN),
        (
            "app/controllers/application_controller.rb",
            "class ApplicationController < ActionController::Base\nend\n",
        ),
        (
            "components/api/app/controllers/rest/v2/application_controller.rb",
            "module Rest\n  module V2\n    class ApplicationController < ApplicationController\n      include Permissions\n    end\n  end\nend\n",
        ),
        (
            "components/auth/app/controllers/rest/v2/internal/domains_controller.rb",
            "module Rest\n  module V2\n    module Internal\n      class DomainsController < Rest::V2::ApplicationController\n        permit_with FooPolicy\n\n        def index\n        end\n      end\n    end\n  end\nend\n",
        ),
    ]))
    .expect("ingest");
    // The component-scoped base controller itself must be ingested —
    // not just reachable as a generic library class.
    assert!(
        app.controllers.iter().any(|c| c.name.0.as_str() == "Rest::V2::ApplicationController"),
        "expected Rest::V2::ApplicationController to be ingested as a Controller: {:?}",
        app.controllers.iter().map(|c| c.name.0.as_str()).collect::<Vec<_>>()
    );
    let c = controller(&app, "Rest::V2::Internal::DomainsController");
    let fs = filters(c);
    assert_eq!(
        fs.iter().filter(|f| f.kind == FilterKind::Around).count(),
        1,
        "permit_with's around_action should resolve through the component-scoped ancestor: {fs:?}"
    );
}
