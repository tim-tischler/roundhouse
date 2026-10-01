//! `class << self … end` in a controller's own class body.
//!
//! Model ingest expands this into class methods
//! (`ingest::model::ingest_singleton_class_methods`); a controller's
//! `class << self` used to reach controller ingest's generic
//! expression catch-all instead, which has no arm for
//! `SingletonClassNode` and refuses with "unsupported expression
//! node: SingletonClassNode" — in survey mode this degrades to one
//! gap line, but `LegacyBase`-shaped base controllers (an ancestor of
//! nearly every controller in a real app) turn that one gap into a
//! diagnostic-downgrading blanket over every descendant.
//!
//! This reuses the model path's tolerant parsing (`ingest_singleton_
//! class_methods`) and files the result on `Controller::class_methods`
//! — a side list, not part of `body`, since `Action`/`ControllerBodyItem`
//! has no class-receiver variant to interleave it into (see the
//! field's doc comment in `src/dialect.rs`). `analyze::harvest_method_
//! returns` registers those methods into the class registry so
//! `self.class.<method>` dispatches, the same way a library class's
//! `MethodReceiver::Class` methods already do.

use std::collections::HashMap;
use std::path::PathBuf;

use roundhouse::analyze::{diagnose, Analyzer};
use roundhouse::dialect::{ControllerBodyItem, MethodReceiver};
use roundhouse::ingest::ingest_app_from_tree;

const APPLICATION_CONTROLLER: &str = r#"class ApplicationController < ActionController::Base
  class << self
    def project_area?
      name.include?('ProjectArea')
    end

    def company_area?
      name.include?('CompanyArea')
    end
  end
end
"#;

const WIDGETS_CONTROLLER: &str = r#"class WidgetsController < ApplicationController
  before_action :set_project

  def index
  end

  private

  def set_project
    @in_project_area = self.class.project_area?
  end
end
"#;

fn tree() -> HashMap<PathBuf, Vec<u8>> {
    let files: Vec<(&str, &str)> = vec![
        ("db/schema.rb", "ActiveRecord::Schema.define do\nend\n"),
        ("app/controllers/application_controller.rb", APPLICATION_CONTROLLER),
        ("app/controllers/widgets_controller.rb", WIDGETS_CONTROLLER),
        (
            "config/routes.rb",
            "Rails.application.routes.draw do\n  resources :widgets, only: [:index]\nend\n",
        ),
    ];
    files.into_iter().map(|(p, c)| (PathBuf::from(p), c.as_bytes().to_vec())).collect()
}

/// Strict (non-survey) ingest must succeed at all: the old behavior
/// was an `IngestError::Unsupported` abort ("unsupported expression
/// node: SingletonClassNode") for the whole file.
#[test]
fn a_controllers_singleton_class_block_ingests_without_error() {
    let app = ingest_app_from_tree(tree()).expect("ingest must not abort on `class << self`");
    let app_controller = app
        .controllers
        .iter()
        .find(|c| c.name.0.as_str() == "ApplicationController")
        .expect("ApplicationController");
    let names: Vec<&str> =
        app_controller.class_methods.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["project_area?", "company_area?"],
        "both singleton-block defs land in Controller::class_methods: {names:?}"
    );
    assert!(
        app_controller.class_methods.iter().all(|m| m.receiver == MethodReceiver::Class),
        "every method pulled from `class << self` is class-receiver: {:?}",
        app_controller.class_methods
    );
    // Not part of the interleaved body — no stray Unknown/Action item
    // for the singleton block itself.
    assert!(
        app_controller.body.iter().all(|item| !matches!(item, ControllerBodyItem::Unknown { .. })),
        "the singleton block must not fall through as an Unknown body item: {:?}",
        app_controller.body
    );
}

/// Survey mode (what a whole-app scope-estimation run uses) must not
/// record a `SingletonClassNode` gap for this file anymore.
#[test]
fn survey_mode_has_no_singleton_class_node_gap() {
    roundhouse::ingest::survey::activate();
    let _app = ingest_app_from_tree(tree()).expect("ingest");
    let gaps = roundhouse::ingest::survey::drain();
    let messages: Vec<String> = gaps.iter().map(|g| format!("{g:?}")).collect();
    assert!(
        !messages.iter().any(|m| m.contains("SingletonClassNode")),
        "no gap should mention SingletonClassNode anymore: {messages:?}"
    );
}

/// `self.class.project_area?`, called from a leaf controller's filter
/// method, must dispatch — not fail to resolve. Before this fix,
/// `ApplicationController` had no `class_methods` entry for
/// `project_area?` at all (the singleton block was never ingested),
/// so the call resolved to nothing.
#[test]
fn self_class_dot_method_from_the_singleton_block_dispatches() {
    let mut app = ingest_app_from_tree(tree()).expect("ingest");
    Analyzer::new(&app).analyze(&mut app);
    let failures: Vec<String> = diagnose(&app)
        .iter()
        .filter(|d| d.code() == "send_dispatch_failed")
        .map(|d| format!("{d:?}"))
        .collect();
    assert!(
        failures.is_empty(),
        "`self.class.project_area?` must dispatch against the registered class method: {failures:?}"
    );
}
