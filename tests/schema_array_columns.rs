//! `t.string :tags, array: true` (and other `array: true` columns) in
//! `db/schema.rb` — the schema.rb-side twin of `structure.sql`'s
//! `text[]` / `bigint[]` support (see `tests/structure_sql.rs`'s
//! `array_columns_are_modeled_not_ledgered`). Before this, `array:
//! true` was simply ignored: the column ingested as its base scalar
//! type (`t.string :tags, array: true` modeled as plain `String`),
//! silently losing the fact that it is Postgres's array wrapper
//! around that type.

use roundhouse::emit::shared::schema_sql::render_schema_statements;
use roundhouse::ingest::ingest_schema;
use roundhouse::schema::ColumnType;

fn schema(schema_rb: &str) -> roundhouse::schema::Schema {
    ingest_schema(schema_rb.as_bytes(), "db/schema.rb").expect("ingest schema")
}

#[test]
fn array_true_wraps_the_base_type() {
    let schema = schema(
        r#"ActiveRecord::Schema[8.1].define(version: 1) do
  create_table "widgets", force: :cascade do |t|
    t.string "tags", array: true
    t.bigint "counts", array: true
    t.string "labels", limit: 255, array: true
    t.string "name"
  end
end
"#,
    );
    let widgets = &schema.tables[&roundhouse::Symbol::from("widgets")];
    let col = |name: &str| {
        widgets.columns.iter().find(|c| c.name.as_str() == name).unwrap_or_else(|| panic!("no column {name}"))
    };

    assert_eq!(col("tags").col_type, ColumnType::Array { elem: Box::new(ColumnType::String { limit: None }) });
    assert_eq!(col("counts").col_type, ColumnType::Array { elem: Box::new(ColumnType::BigInt) });
    // `limit:` still applies to the wrapped element type.
    assert_eq!(
        col("labels").col_type,
        ColumnType::Array { elem: Box::new(ColumnType::String { limit: Some(255) }) }
    );
    // A plain (non-array) column of the same base type is unaffected.
    assert_eq!(col("name").col_type, ColumnType::String { limit: None });
}

/// The DDL renderer has no SQLite array type — `array: true` stores as
/// TEXT (JSON-encoded), the same seam as `jsonb`.
#[test]
fn array_true_renders_as_sqlite_text() {
    let schema = schema(
        r#"ActiveRecord::Schema[8.1].define(version: 1) do
  create_table "widgets", force: :cascade do |t|
    t.string "tags", array: true, null: false
  end
end
"#,
    );
    let out = render_schema_statements(&schema);
    assert_eq!(
        out[0],
        "CREATE TABLE IF NOT EXISTS widgets (\n  id INTEGER PRIMARY KEY AUTOINCREMENT,\n  \
         tags TEXT NOT NULL\n)"
    );
}
