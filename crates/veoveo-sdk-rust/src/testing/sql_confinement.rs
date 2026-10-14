//! The schema confinement of a server's SQL.
//!
//! A server's SQL names only its own schema. Core keeps its state, such as Tasks, process
//! instances, usage and audit, in the `tasks`, `kernel`, `usage`, `audit`, `identity` and
//! `gateway` schemas, and a server reaches that state only through this SDK and core's
//! runtime API. The check reads every SQL source of a crate with PostgreSQL's own parser,
//! from libpg_query, so comments and string literals never count as names:
//!
//! - every `.sql` file under the crate root, which holds its query files and migrations;
//! - the SQL literal of every sqlx query macro and every `sqlx` query function in its
//!   `.rs` sources, and the file of every `query_file` macro;
//! - the body of every PL/pgSQL function and `DO` block, through libpg_query's PL/pgSQL
//!   parser, and of every SQL-language function.
//!
//! It skips `target` and every hidden directory, so the `.sqlx` offline metadata, which
//! holds core's own queries, never counts. SQL the check cannot read fails it: a statement
//! the parser refuses, SQL built at run time, and a function in another language.

use std::{
    collections::BTreeSet,
    fmt, fs,
    path::{Path, PathBuf},
};

use pg_query::protobuf::ObjectType;
use serde_json::Value;
use syn::{
    Expr, ExprCall, ExprLit, ExprMethodCall, ItemUse, Lit, LitStr, Macro, Token, UseTree,
    punctuated::Punctuated,
    spanned::Spanned,
    visit::{self, Visit},
};

/// Schemas every server may name: PostgreSQL's catalog, which the parser also names for
/// built-in types, and the session's temporary schema.
const SYSTEM_SCHEMAS: [&str; 2] = ["pg_catalog", "pg_temp"];

/// Core functions a server may name, and only as the function a trigger executes.
/// `kernel.notify_change` wakes the listeners of the table's schema channel.
const TRIGGER_FUNCTIONS: [(&str, &str); 1] = [("kernel", "notify_change")];

/// sqlx macros whose first string literal is SQL.
const SQL_MACROS: [&str; 6] = [
    "query",
    "query_as",
    "query_scalar",
    "query_unchecked",
    "query_as_unchecked",
    "query_scalar_unchecked",
];

/// sqlx macros whose first string literal names an SQL file relative to the crate root.
const SQL_FILE_MACROS: [&str; 6] = [
    "query_file",
    "query_file_as",
    "query_file_scalar",
    "query_file_unchecked",
    "query_file_as_unchecked",
    "query_file_scalar_unchecked",
];

/// sqlx functions whose first argument is SQL.
const SQL_FUNCTIONS: [&str; 5] = ["query", "query_as", "query_scalar", "query_with", "raw_sql"];

/// sqlx executor methods that run a string argument as SQL.
const SQL_METHODS: [&str; 10] = [
    "execute",
    "execute_many",
    "fetch",
    "fetch_all",
    "fetch_many",
    "fetch_one",
    "fetch_optional",
    "prepare",
    "prepare_with",
    "describe",
];

/// The dollar quote that wraps a `DO` block's body for the PL/pgSQL parser.
const DO_QUOTE: &str = "$veoveo_do_block$";

/// Why SQL fails the check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlProblem {
    /// The SQL names `name`, which lies in `schema`.
    ForeignSchema { schema: String, name: String },
    /// A `search_path` setting resolves unqualified names in `schema`.
    SearchPath { schema: String },
    /// The SQL is built at run time, so the check cannot read it.
    DynamicSql,
    /// A function in `language`, whose body the check cannot read.
    UncheckedLanguage { language: String },
    /// The parser refused the SQL or the Rust source that holds it.
    Unparsed { reason: String },
}

/// One place where a crate's SQL fails the check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SqlFinding {
    /// The file, relative to the crate root when it lies under it.
    pub path: PathBuf,
    /// The 1-based line of the statement or name.
    pub line: usize,
    pub problem: SqlProblem,
}

impl fmt::Display for SqlFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}: ", self.path.display(), self.line)?;
        match &self.problem {
            SqlProblem::ForeignSchema { schema, name } => {
                write!(f, "`{name}` lies in schema `{schema}`")
            }
            SqlProblem::SearchPath { schema } => {
                write!(f, "search_path names schema `{schema}`")
            }
            SqlProblem::DynamicSql => write!(f, "SQL built at run time cannot be checked"),
            SqlProblem::UncheckedLanguage { language } => {
                write!(f, "a function in language `{language}` cannot be checked")
            }
            SqlProblem::Unparsed { reason } => write!(f, "cannot be parsed: {reason}"),
        }
    }
}

/// Fail the calling test when SQL under `crate_root` names any schema but `own_schema`.
///
/// Run it as an ordinary test of a server crate:
///
/// ```no_run
/// #[test]
/// fn sql_names_only_the_servers_schema() {
///     veoveo_sdk_rust::testing::assert_sql_confined(env!("CARGO_MANIFEST_DIR"), "media");
/// }
/// ```
pub fn assert_sql_confined(crate_root: impl AsRef<Path>, own_schema: &str) {
    let findings = check_sql_confined(crate_root.as_ref(), own_schema);
    if findings.is_empty() {
        return;
    }
    let mut report = format!(
        "SQL under {} must name only the schema `{own_schema}`. A server reads and changes \
         core state, such as its Tasks, only through veoveo-sdk-rust and core's runtime \
         API. When that API lacks something, ask for the feature in veoveo-core or \
         veoveo-sdk-rust instead of querying core's tables.\n",
        crate_root.as_ref().display()
    );
    for finding in &findings {
        report.push_str(&format!("  {finding}\n"));
    }
    panic!("{report}");
}

/// The places where SQL under `crate_root` names a schema other than `own_schema`, or
/// cannot be checked. An empty list means the crate's SQL is confined.
pub fn check_sql_confined(crate_root: impl AsRef<Path>, own_schema: &str) -> Vec<SqlFinding> {
    let root = crate_root.as_ref();
    let mut sources = Sources::default();
    sources.walk(root, root);
    let mut checker = Checker {
        root,
        own: own_schema,
        findings: Vec::new(),
    };
    for failure in sources.failures.drain(..) {
        checker.findings.push(failure);
    }
    let mut sql_files = sources.sql.clone();
    for path in &sources.rust {
        checker.rust_file(path, &mut sql_files);
    }
    for path in &sql_files {
        match fs::read_to_string(path) {
            Ok(text) => checker.sql(&text, &Origin::new(root, path, 1)),
            Err(error) => checker.push(
                &Origin::new(root, path, 1),
                1,
                SqlProblem::Unparsed {
                    reason: error.to_string(),
                },
            ),
        }
    }
    checker
        .findings
        .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    checker.findings.dedup();
    checker.findings
}

/// The `.sql` and `.rs` files under a crate root.
#[derive(Default)]
struct Sources {
    sql: BTreeSet<PathBuf>,
    rust: BTreeSet<PathBuf>,
    failures: Vec<SqlFinding>,
}

impl Sources {
    fn walk(&mut self, root: &Path, dir: &Path) {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                self.failures.push(SqlFinding {
                    path: relative(root, dir),
                    line: 1,
                    problem: SqlProblem::Unparsed {
                        reason: error.to_string(),
                    },
                });
                return;
            }
        };
        let mut paths = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if !name.starts_with('.') && name != "target" {
                    self.walk(root, &path);
                }
            } else if name.ends_with(".sql") {
                self.sql.insert(path);
            } else if name.ends_with(".rs") {
                self.rust.insert(path);
            }
        }
    }
}

fn relative(root: &Path, path: &Path) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

/// Where a piece of SQL starts: its file and the line of its first character.
struct Origin {
    path: PathBuf,
    line: usize,
}

impl Origin {
    fn new(root: &Path, path: &Path, line: usize) -> Self {
        Self {
            path: relative(root, path),
            line,
        }
    }

    /// The line of byte `offset` into `sql`, which starts at this origin.
    fn line_at(&self, sql: &str, offset: usize) -> usize {
        let offset = offset.min(sql.len());
        let before = sql.get(..offset).unwrap_or(sql);
        self.line + before.matches('\n').count()
    }
}

struct Checker<'a> {
    root: &'a Path,
    own: &'a str,
    findings: Vec<SqlFinding>,
}

impl Checker<'_> {
    fn push(&mut self, origin: &Origin, line: usize, problem: SqlProblem) {
        self.findings.push(SqlFinding {
            path: origin.path.clone(),
            line,
            problem,
        });
    }

    fn allowed(&self, schema: &str) -> bool {
        schema == self.own || SYSTEM_SCHEMAS.contains(&schema)
    }

    /// The SQL of one Rust source: its query macros and calls, and the files its
    /// `query_file` macros name, which join `sql_files`.
    fn rust_file(&mut self, path: &Path, sql_files: &mut BTreeSet<PathBuf>) {
        let origin = Origin::new(self.root, path, 1);
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) => {
                return self.push(
                    &origin,
                    1,
                    SqlProblem::Unparsed {
                        reason: error.to_string(),
                    },
                );
            }
        };
        let file = match syn::parse_file(&text) {
            Ok(file) => file,
            Err(error) => {
                return self.push(
                    &origin,
                    error.span().start().line.max(1),
                    SqlProblem::Unparsed {
                        reason: format!("the Rust source: {error}"),
                    },
                );
            }
        };
        let mut found = RustSql::default();
        found.visit_file(&file);
        for (line, problem) in found.problems {
            self.push(&origin, line, problem);
        }
        for literal in found.sql {
            self.sql(
                &literal.value(),
                &Origin::new(self.root, path, literal.span().start().line.max(1)),
            );
        }
        for literal in found.files {
            let named = self.root.join(literal.value());
            if named.is_file() {
                sql_files.insert(named);
            } else {
                self.push(
                    &origin,
                    literal.span().start().line.max(1),
                    SqlProblem::Unparsed {
                        reason: format!("no SQL file at {}", literal.value()),
                    },
                );
            }
        }
    }

    /// Check one piece of SQL that starts at `origin`.
    fn sql(&mut self, sql: &str, origin: &Origin) {
        let parsed = match pg_query::parse(sql) {
            Ok(parsed) => parsed,
            Err(error) => {
                return self.push(
                    origin,
                    origin.line,
                    SqlProblem::Unparsed {
                        reason: error.to_string(),
                    },
                );
            }
        };
        for raw in &parsed.protobuf.stmts {
            let start = first_token(sql, usize::try_from(raw.stmt_location).unwrap_or(0));
            let length = usize::try_from(raw.stmt_len).unwrap_or(0);
            let text = match length {
                0 => sql.get(start..).unwrap_or(sql),
                length => sql.get(start..start + length).unwrap_or(sql),
            };
            let tree = match serde_json::to_value(&raw.stmt) {
                Ok(tree) => tree,
                Err(error) => {
                    self.push(
                        origin,
                        origin.line_at(sql, start),
                        SqlProblem::Unparsed {
                            reason: error.to_string(),
                        },
                    );
                    continue;
                }
            };
            let mut statement = Statement {
                checker: self,
                sql,
                origin,
                start,
                text,
            };
            statement.walk(&tree);
        }
    }
}

/// One statement of a piece of SQL, while its parse tree is walked.
struct Statement<'c, 'a, 's> {
    checker: &'c mut Checker<'a>,
    sql: &'s str,
    origin: &'s Origin,
    /// The statement's byte offset into `sql`.
    start: usize,
    /// The statement's own text.
    text: &'s str,
}

/// What a qualified name names, which decides the names a server may use.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NameUse {
    Object,
    TriggerFunction,
}

impl Statement<'_, '_, '_> {
    fn line(&self, location: Option<i64>) -> usize {
        let offset = location
            .and_then(|location| usize::try_from(location).ok())
            .unwrap_or(self.start);
        self.origin.line_at(self.sql, offset)
    }

    fn push(&mut self, location: Option<i64>, problem: SqlProblem) {
        let line = self.line(location);
        self.checker.push(self.origin, line, problem);
    }

    fn schema(&mut self, schema: &str, name: &str, location: Option<i64>) {
        if schema.is_empty() || self.checker.allowed(schema) {
            return;
        }
        self.push(
            location,
            SqlProblem::ForeignSchema {
                schema: schema.to_owned(),
                name: name.to_owned(),
            },
        );
    }

    /// A possibly qualified name: `name`, `schema.name` or `catalog.schema.name`.
    fn name(&mut self, parts: &[String], location: Option<i64>, used: NameUse) {
        if parts.len() < 2 {
            return;
        }
        let schema = &parts[parts.len() - 2];
        let object = &parts[parts.len() - 1];
        if used == NameUse::TriggerFunction
            && TRIGGER_FUNCTIONS
                .iter()
                .any(|(allowed_schema, function)| schema == allowed_schema && object == function)
        {
            return;
        }
        self.schema(schema, &parts.join("."), location);
    }

    fn walk(&mut self, value: &Value) {
        match value {
            Value::Object(map) => {
                self.shape(value);
                for (key, inner) in map {
                    // A column reference names a table or alias and a column, never a
                    // schema the statement reads.
                    if key == "ColumnRef" {
                        continue;
                    }
                    self.node(key, inner);
                    self.walk(inner);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.walk(item);
                }
            }
            _ => {}
        }
    }

    /// A relation, type, function call or function reference, known by its fields. A
    /// statement holds some of them directly, such as an `UPDATE`'s relation or a foreign
    /// key's referenced table, and others inside a node envelope, so the walk recognizes
    /// each by its shape wherever it appears.
    fn shape(&mut self, node: &Value) {
        let has = |field: &str| node.get(field).is_some();
        let location = node.get("location").and_then(Value::as_i64);
        if has("relname") && has("schemaname") {
            let schema = text_field(node, "schemaname");
            let relation = text_field(node, "relname");
            self.schema(&schema, &format!("{schema}.{relation}"), location);
        } else if has("names") && has("type_oid") {
            let parts = names(node.get("names"));
            self.name(&parts, location, NameUse::Object);
        } else if has("objname") && has("objargs") {
            let parts = names(node.get("objname"));
            self.name(&parts, None, NameUse::Object);
        } else if has("funcname") && has("agg_order") {
            let parts = names(node.get("funcname"));
            self.name(&parts, location, NameUse::Object);
        }
    }

    fn node(&mut self, kind: &str, node: &Value) {
        let location = node.get("location").and_then(Value::as_i64);
        match kind {
            "CreateFunctionStmt" => {
                let parts = names(node.get("funcname"));
                self.name(&parts, None, NameUse::Object);
                self.function(node);
            }
            "CreateTrigStmt" => {
                let parts = names(node.get("funcname"));
                self.name(&parts, None, NameUse::TriggerFunction);
            }
            "DropStmt"
            | "CommentStmt"
            | "AlterOwnerStmt"
            | "RenameStmt"
            | "SecLabelStmt"
            | "AlterObjectSchemaStmt" => self.objects(node),
            "GrantStmt" => {
                if object_type(node, "objtype") == Some(ObjectType::ObjectSchema) {
                    for schema in strings(node.get("objects")) {
                        self.schema(&schema, &schema, None);
                    }
                }
            }
            "CreateSchemaStmt" => {
                let schema = text_field(node, "schemaname");
                self.schema(&schema, &schema, None);
            }
            "DefineStmt" | "CreateStatsStmt" => {
                let parts = names(node.get("defnames"));
                self.name(&parts, None, NameUse::Object);
            }
            "CreateEnumStmt" | "AlterEnumStmt" => {
                let parts = names(node.get("type_name"));
                self.name(&parts, None, NameUse::Object);
            }
            "CreateDomainStmt" => {
                let parts = names(node.get("domainname"));
                self.name(&parts, None, NameUse::Object);
            }
            "AlterDefaultPrivilegesStmt" => {
                for option in nodes(node.get("options")) {
                    if let Some(element) = option.get("DefElem")
                        && text_field(element, "defname") == "schemas"
                    {
                        for schema in strings(element.get("arg").and_then(list_items)) {
                            self.schema(&schema, &schema, None);
                        }
                    }
                }
            }
            "VariableSetStmt" => {
                if text_field(node, "name").eq_ignore_ascii_case("search_path") {
                    for argument in nodes(node.get("args")) {
                        let Some(schema) = argument
                            .get("AConst")
                            .and_then(|constant| constant.get("val"))
                            .and_then(|val| val.get("Sval"))
                            .map(|sval| text_field(sval, "sval"))
                        else {
                            continue;
                        };
                        if !self.checker.allowed(&schema) {
                            self.push(location, SqlProblem::SearchPath { schema });
                        }
                    }
                }
            }
            "DoStmt" => self.do_block(node),
            _ => {}
        }
    }

    /// The objects a `DROP`, `COMMENT`, `ALTER … OWNER`, `ALTER … RENAME`, `SECURITY LABEL`
    /// or `ALTER … SET SCHEMA` names. A schema object is a plain name; other objects are
    /// qualified names.
    fn objects(&mut self, node: &Value) {
        let kind = ["remove_type", "objtype", "object_type", "rename_type"]
            .iter()
            .find_map(|field| object_type(node, field));
        let schema_object = kind == Some(ObjectType::ObjectSchema);
        // A trigger, policy, rule, constraint or column follows its table's name.
        let table_member = kind.is_some_and(|kind| {
            [
                ObjectType::ObjectTrigger,
                ObjectType::ObjectPolicy,
                ObjectType::ObjectRule,
                ObjectType::ObjectTabconstraint,
                ObjectType::ObjectColumn,
            ]
            .contains(&kind)
        });
        let mut objects = nodes(node.get("objects")).collect::<Vec<_>>();
        if let Some(object) = node.get("object") {
            objects.push(inner(object));
        }
        for object in objects {
            if let Some(items) = list_items(object) {
                let mut parts = strings(Some(items));
                if table_member {
                    parts.pop();
                }
                self.name(&parts, None, NameUse::Object);
            } else if schema_object && let Some(text) = object.get("String") {
                let schema = text_field(text, "sval");
                self.schema(&schema, &schema, None);
            }
        }
        let new_schema = text_field(node, "newschema");
        self.schema(&new_schema, &new_schema, None);
    }

    /// The body of a function definition, in its language.
    fn function(&mut self, node: &Value) {
        let mut language = String::from("sql");
        let mut body = None;
        for option in nodes(node.get("options")) {
            let Some(element) = option.get("DefElem") else {
                continue;
            };
            match text_field(element, "defname").as_str() {
                "language" => {
                    if let Some(text) = element
                        .get("arg")
                        .map(inner)
                        .and_then(|arg| arg.get("String"))
                    {
                        language = text_field(text, "sval");
                    }
                }
                "as" => {
                    body = strings(element.get("arg").and_then(list_items))
                        .into_iter()
                        .next();
                }
                _ => {}
            }
        }
        match language.as_str() {
            // A `BEGIN ATOMIC` body is part of the parse tree the walk reads.
            "sql" => {
                if let Some(body) = body {
                    let origin = Origin {
                        path: self.origin.path.clone(),
                        line: self.line(None),
                    };
                    self.checker.sql(&body, &origin);
                }
            }
            "plpgsql" => {
                let text = self.text.to_owned();
                self.plpgsql(&text);
            }
            other => self.push(
                None,
                SqlProblem::UncheckedLanguage {
                    language: other.to_owned(),
                },
            ),
        }
    }

    /// A `DO` block, whose body the PL/pgSQL parser reads as a function's.
    fn do_block(&mut self, node: &Value) {
        let mut language = String::from("plpgsql");
        let mut body = None;
        for argument in nodes(node.get("args")) {
            let Some(element) = argument.get("DefElem") else {
                continue;
            };
            let value = element
                .get("arg")
                .map(inner)
                .and_then(|arg| arg.get("String"))
                .map(|text| text_field(text, "sval"));
            match text_field(element, "defname").as_str() {
                "language" => language = value.unwrap_or(language),
                "as" => body = value,
                _ => {}
            }
        }
        if language != "plpgsql" {
            return self.push(None, SqlProblem::UncheckedLanguage { language });
        }
        let Some(body) = body else {
            return;
        };
        if body.contains(DO_QUOTE) {
            return self.push(
                None,
                SqlProblem::Unparsed {
                    reason: format!("the DO block contains {DO_QUOTE}"),
                },
            );
        }
        self.plpgsql(&format!(
            "CREATE FUNCTION pg_temp.veoveo_do_block() RETURNS void LANGUAGE plpgsql AS \
             {DO_QUOTE}{body}{DO_QUOTE}"
        ));
    }

    /// A PL/pgSQL function definition: every SQL statement and expression in its body.
    fn plpgsql(&mut self, definition: &str) {
        match pg_query::parse_plpgsql(definition) {
            Ok(tree) => self.plpgsql_walk(&tree),
            Err(error) => self.push(
                None,
                SqlProblem::Unparsed {
                    reason: format!("the PL/pgSQL body: {error}"),
                },
            ),
        }
    }

    fn plpgsql_walk(&mut self, value: &Value) {
        match value {
            Value::Object(map) => {
                for (key, inner) in map {
                    match key.as_str() {
                        "PLpgSQL_expr" => {
                            if let Some(query) = inner.get("query").and_then(Value::as_str) {
                                self.embedded(query);
                            }
                        }
                        "PLpgSQL_stmt_dynexecute" | "PLpgSQL_stmt_dynfors" => {
                            self.push(None, SqlProblem::DynamicSql);
                        }
                        "PLpgSQL_stmt_open" | "PLpgSQL_stmt_return_query" => {
                            if inner.get("dynquery").is_some() {
                                self.push(None, SqlProblem::DynamicSql);
                            }
                        }
                        "PLpgSQL_type" => {
                            if let Some(name) = inner.get("typname").and_then(Value::as_str) {
                                let parts = type_name_parts(name);
                                self.name(&parts, None, NameUse::Object);
                            }
                        }
                        _ => {}
                    }
                    self.plpgsql_walk(inner);
                }
            }
            Value::Array(items) => {
                for item in items {
                    self.plpgsql_walk(item);
                }
            }
            _ => {}
        }
    }

    /// One statement or expression of a PL/pgSQL body. An assignment target precedes
    /// `:=`, and an expression reads as the select list of a query.
    fn embedded(&mut self, query: &str) {
        let origin = Origin {
            path: self.origin.path.clone(),
            line: self.line(None),
        };
        let mut candidates = vec![query.to_owned(), format!("SELECT {query}")];
        if let Some(value) = assignment_value(query) {
            candidates.push(format!("SELECT {value}"));
        }
        let Some(readable) = candidates
            .iter()
            .find(|candidate| pg_query::parse(candidate).is_ok())
        else {
            return self.push(
                None,
                SqlProblem::Unparsed {
                    reason: format!("the PL/pgSQL statement `{query}`"),
                },
            );
        };
        self.checker.sql(readable, &origin);
    }
}

/// The offset of the first token at or after `offset`, past whitespace and comments. A
/// statement's location begins right after the previous statement's semicolon.
fn first_token(sql: &str, offset: usize) -> usize {
    let mut at = offset.min(sql.len());
    loop {
        let rest = sql.get(at..).unwrap_or_default();
        let trimmed = rest.trim_start();
        at += rest.len() - trimmed.len();
        if trimmed.starts_with("--") {
            at += trimmed.find('\n').unwrap_or(trimmed.len());
        } else if let Some(comment) = trimmed.strip_prefix("/*") {
            at += 2 + comment.find("*/").map_or(comment.len(), |end| end + 2);
        } else {
            return at;
        }
    }
}

/// The value of a PL/pgSQL assignment, `target := value` or `target = value`, whose
/// target is a variable, a field or an array element.
fn assignment_value(query: &str) -> Option<&str> {
    let (target, value) = query.split_once(":=").or_else(|| query.split_once('='))?;
    let target = target.trim();
    let is_target = !target.is_empty()
        && target
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | '"'));
    is_target.then_some(value)
}

/// The parts of a PL/pgSQL declared type such as `artifact.level` or
/// `tasks.task%ROWTYPE`, folded to lower case unless quoted.
fn type_name_parts(name: &str) -> Vec<String> {
    let name = name.split('%').next().unwrap_or(name);
    let name = name.split('(').next().unwrap_or(name).trim();
    let name = name.strip_suffix("[]").unwrap_or(name);
    name.split('.')
        .map(|part| {
            let part = part.trim();
            match part
                .strip_prefix('"')
                .and_then(|part| part.strip_suffix('"'))
            {
                Some(quoted) => quoted.to_owned(),
                None => part.to_lowercase(),
            }
        })
        .collect()
}

/// A node unwrapped from its `node` envelope.
fn inner(value: &Value) -> &Value {
    value.get("node").unwrap_or(value)
}

fn text_field(node: &Value, field: &str) -> String {
    node.get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn object_type(node: &Value, field: &str) -> Option<ObjectType> {
    node.get(field)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .and_then(|value| ObjectType::try_from(value).ok())
}

/// The child nodes of a repeated node field, each unwrapped from its `node` envelope.
fn nodes(field: Option<&Value>) -> impl Iterator<Item = &Value> {
    field
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("node"))
}

/// The items of a `List` node.
fn list_items(node: &Value) -> Option<&Value> {
    node.get("node")
        .unwrap_or(node)
        .get("List")
        .and_then(|list| list.get("items"))
}

/// The text of each `String` node in a repeated node field.
fn strings(field: Option<&Value>) -> Vec<String> {
    nodes(field)
        .filter_map(|node| node.get("String"))
        .map(|text| text_field(text, "sval"))
        .collect()
}

/// A qualified name as the parser keeps it: a repeated field of `String` nodes.
fn names(field: Option<&Value>) -> Vec<String> {
    strings(field)
}

/// The SQL a Rust source holds in sqlx macros and calls.
#[derive(Default)]
struct RustSql {
    sql: Vec<LitStr>,
    files: Vec<LitStr>,
    problems: Vec<(usize, SqlProblem)>,
    /// sqlx query functions this file imports under a single name.
    imported: BTreeSet<String>,
}

fn is_sql_function(name: &str) -> bool {
    SQL_FUNCTIONS.contains(&name)
}

fn first_literal<'a>(arguments: impl IntoIterator<Item = &'a Expr>) -> Option<LitStr> {
    arguments.into_iter().find_map(|argument| match argument {
        Expr::Lit(ExprLit {
            lit: Lit::Str(literal),
            ..
        }) => Some(literal.clone()),
        _ => None,
    })
}

impl RustSql {
    fn imports(&mut self, tree: &UseTree, from_sqlx: bool) {
        match tree {
            UseTree::Path(path) => {
                let sqlx = from_sqlx || path.ident == "sqlx";
                self.imports(&path.tree, sqlx);
            }
            UseTree::Name(name) if from_sqlx && is_sql_function(&name.ident.to_string()) => {
                self.imported.insert(name.ident.to_string());
            }
            UseTree::Rename(rename) if from_sqlx && is_sql_function(&rename.ident.to_string()) => {
                self.imported.insert(rename.rename.to_string());
            }
            UseTree::Group(group) => {
                for tree in &group.items {
                    self.imports(tree, from_sqlx);
                }
            }
            _ => {}
        }
    }
}

impl<'ast> Visit<'ast> for RustSql {
    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        self.imports(&item.tree, false);
        visit::visit_item_use(self, item);
    }

    fn visit_macro(&mut self, mac: &'ast Macro) {
        let name = mac
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            .unwrap_or_default();
        let sql = SQL_MACROS.contains(&name.as_str());
        let file = SQL_FILE_MACROS.contains(&name.as_str());
        if sql || file {
            let line = mac.span().start().line.max(1);
            match mac.parse_body_with(Punctuated::<Expr, Token![,]>::parse_terminated) {
                Ok(arguments) => match first_literal(&arguments) {
                    Some(literal) if sql => self.sql.push(literal),
                    Some(literal) => self.files.push(literal),
                    None => self.problems.push((line, SqlProblem::DynamicSql)),
                },
                Err(error) => self.problems.push((
                    line,
                    SqlProblem::Unparsed {
                        reason: format!("the arguments of {name}!: {error}"),
                    },
                )),
            }
        }
        visit::visit_macro(self, mac);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(path) = &*call.func {
            let segments = &path.path.segments;
            let last = segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            let through_sqlx = segments.len() > 1
                && segments
                    .first()
                    .is_some_and(|segment| segment.ident == "sqlx")
                && SQL_FUNCTIONS.contains(&last.as_str());
            let imported = segments.len() == 1 && self.imported.contains(&last);
            if through_sqlx || imported {
                match first_literal(call.args.iter().take(1)) {
                    Some(literal) => self.sql.push(literal),
                    None => self
                        .problems
                        .push((call.span().start().line.max(1), SqlProblem::DynamicSql)),
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if SQL_METHODS.contains(&call.method.to_string().as_str())
            && let Some(literal) = first_literal(call.args.iter().take(1))
        {
            self.sql.push(literal);
        }
        visit::visit_expr_method_call(self, call);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A crate tree in a fresh temporary directory, removed when the test ends.
    struct Fixture(PathBuf);

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let root =
                std::env::temp_dir().join(format!("sql-confinement-{}", uuid::Uuid::new_v4()));
            for (path, text) in files {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, text).unwrap();
            }
            Self(root)
        }

        fn findings(&self) -> Vec<SqlFinding> {
            check_sql_confined(&self.0, "media")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn foreign(findings: &[SqlFinding]) -> Vec<(String, usize, String)> {
        findings
            .iter()
            .map(|finding| match &finding.problem {
                SqlProblem::ForeignSchema { schema, .. } => (
                    finding.path.display().to_string(),
                    finding.line,
                    schema.clone(),
                ),
                other => panic!("expected a foreign schema, found {other:?} in {finding}"),
            })
            .collect()
    }

    const OWN_MIGRATION: &str = r#"-- Media's tables. Tasks live in core; Media reads them through the SDK, never
-- through tasks.task or kernel.instance.
CREATE TABLE media.prediction (
    tenant uuid NOT NULL,
    task_id uuid NOT NULL,
    model_id text NOT NULL CHECK (model_id <> 'tasks.task'),
    outputs jsonb NOT NULL DEFAULT '[]',
    created_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (tenant, task_id)
);
/* A block comment that mentions audit.record and usage.daily. */
CREATE INDEX prediction_created ON media.prediction (created_at, task_id);

CREATE FUNCTION media.touch() RETURNS trigger
LANGUAGE plpgsql
SET search_path = pg_catalog, pg_temp
AS $$
DECLARE
    stamp timestamptz := now();
    previous media.prediction%ROWTYPE;
BEGIN
    IF TG_OP = 'UPDATE' AND OLD.model_id = 'tasks.task' THEN
        RAISE NOTICE 'tasks.task % is only text', OLD.task_id;
    END IF;
    SELECT * INTO previous FROM media.prediction WHERE task_id = NEW.task_id;
    UPDATE media.prediction SET created_at = stamp WHERE task_id = NEW.task_id;
    RETURN NEW;
END
$$;

CREATE TRIGGER prediction_touch AFTER UPDATE ON media.prediction
    FOR EACH ROW EXECUTE FUNCTION media.touch();
CREATE TRIGGER prediction_notify AFTER INSERT OR UPDATE OR DELETE ON media.prediction
    FOR EACH ROW EXECUTE FUNCTION kernel.notify_change('task_id', 'tenant');
"#;

    const OWN_QUERY: &str = r#"-- veoveo: all tenants (the sweep reads every tenant's rows; tasks.task is core's)
SELECT
    p.task_id AS "task_id: TaskId",
    p.model_id,
    p.outputs AS "outputs: Json<Vec<Output>>",
    $2::interval AS window,
    CAST(p.created_at AS date) AS day
FROM media.prediction AS p
WHERE p.tenant = $1 AND p.task_id = ANY ($3::uuid[]) AND p.model_id <> 'kernel.instance'
ORDER BY p.created_at DESC, p.task_id DESC
"#;

    const OWN_RUST: &str = r#"
use sqlx::query as run_query;
// sqlx::query!("SELECT * FROM tasks.task") in a comment is never SQL.
pub async fn read(pool: &sqlx::PgPool, id: uuid::Uuid) {
    let note = "tasks.task is a name in a string, not a query";
    sqlx::query!("SELECT model_id FROM media.prediction WHERE task_id = $1", id);
    sqlx::query_file_as!(Row, "src/store/by_task.sql", id);
    run_query("DELETE FROM media.prediction WHERE task_id = $1");
    pool.execute("SELECT 1 FROM media.prediction").await;
}
"#;

    #[test]
    fn a_server_naming_only_its_own_schema_passes() {
        let fixture = Fixture::new(&[
            ("migrations/0001_media.sql", OWN_MIGRATION),
            ("src/store/by_task.sql", OWN_QUERY),
            ("src/store.rs", OWN_RUST),
        ]);
        assert_eq!(fixture.findings(), Vec::new());
    }

    #[test]
    fn mentions_in_comments_and_string_literals_pass() {
        let fixture = Fixture::new(&[(
            "src/mention.sql",
            "-- tasks.task\n/* kernel.instance, usage.daily */\nSELECT 'tasks.task', $$audit.record$$ \
             FROM media.prediction -- gateway.route\n",
        )]);
        assert_eq!(fixture.findings(), Vec::new());
    }

    #[test]
    fn core_tables_in_a_query_file_fail() {
        let fixture = Fixture::new(&[(
            "src/store/by_task.sql",
            "SELECT p.task_id, t.status\nFROM media.prediction AS p\nJOIN tasks.task AS t ON t.id = p.task_id\n",
        )]);
        assert_eq!(
            foreign(&fixture.findings()),
            [("src/store/by_task.sql".into(), 3, "tasks".into())]
        );
    }

    #[test]
    fn core_tables_in_an_inline_macro_fail() {
        let fixture = Fixture::new(&[(
            "src/store.rs",
            "fn read() {\n    let rows = sqlx::query_as!(\n        Row,\n        \"SELECT status FROM tasks.task WHERE id = $1\",\n        id\n    );\n}\n",
        )]);
        assert_eq!(
            foreign(&fixture.findings()),
            [("src/store.rs".into(), 4, "tasks".into())]
        );
    }

    #[test]
    fn core_tables_in_a_migration_fail() {
        let fixture = Fixture::new(&[(
            "migrations/0002_link.sql",
            "ALTER TABLE media.prediction\n    ADD FOREIGN KEY (task_id) REFERENCES tasks.task (id);\nGRANT SELECT ON kernel.instance TO media_reader;\n",
        )]);
        assert_eq!(
            foreign(&fixture.findings()),
            [
                ("migrations/0002_link.sql".into(), 2, "tasks".into()),
                ("migrations/0002_link.sql".into(), 3, "kernel".into()),
            ]
        );
    }

    #[test]
    fn a_table_member_is_named_after_its_table() {
        let fixture = Fixture::new(&[(
            "migrations/0006_members.sql",
            "DROP TRIGGER prediction_touch ON media.prediction;
DROP POLICY prediction_owner ON media.prediction;
             COMMENT ON COLUMN media.prediction.model_id IS 'tasks.task';
             COMMENT ON CONSTRAINT prediction_pkey ON media.prediction IS 'own';
             DROP TRIGGER task_notify ON tasks.task;
",
        )]);
        assert_eq!(
            foreign(&fixture.findings()),
            [("migrations/0006_members.sql".into(), 5, "tasks".into())]
        );
    }

    #[test]
    fn other_servers_and_unknown_schemas_fail() {
        let fixture = Fixture::new(&[(
            "src/store/mixed.sql",
            "SELECT a.name FROM artifact.artifact AS a\nUNION ALL SELECT name FROM public.items\nUNION ALL SELECT usage.total(1)::text\n",
        )]);
        let schemas = foreign(&fixture.findings())
            .into_iter()
            .map(|(_, _, schema)| schema)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            schemas,
            BTreeSet::from(["artifact".into(), "public".into(), "usage".into()])
        );
    }

    #[test]
    fn function_bodies_are_checked() {
        let fixture = Fixture::new(&[(
            "migrations/0003_functions.sql",
            "CREATE FUNCTION media.pending() RETURNS bigint LANGUAGE plpgsql AS $$\nDECLARE\n    total bigint := (SELECT count(*) FROM tasks.task);\nBEGIN\n    PERFORM 1 FROM kernel.instance;\n    RETURN total;\nEND\n$$;\n\
             CREATE FUNCTION media.count() RETURNS bigint LANGUAGE sql AS 'SELECT count(*) FROM audit.record';\n\
             DO $$ BEGIN UPDATE usage.daily SET amount = 0; END $$;\n",
        )]);
        let schemas = foreign(&fixture.findings())
            .into_iter()
            .map(|(_, _, schema)| schema)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            schemas,
            BTreeSet::from([
                "audit".into(),
                "kernel".into(),
                "tasks".into(),
                "usage".into()
            ])
        );
    }

    #[test]
    fn a_core_trigger_function_is_allowed_only_in_a_trigger() {
        let fixture = Fixture::new(&[("src/store/notify.sql", "SELECT kernel.notify_change()\n")]);
        assert_eq!(
            foreign(&fixture.findings()),
            [("src/store/notify.sql".into(), 1, "kernel".into())]
        );
    }

    #[test]
    fn search_paths_and_dynamic_sql_fail_closed() {
        let fixture = Fixture::new(&[
            (
                "src/store/path.sql",
                "SET search_path = tasks, pg_catalog;\n",
            ),
            (
                "migrations/0004_dynamic.sql",
                "CREATE FUNCTION media.run(statement text) RETURNS void LANGUAGE plpgsql AS $$\nBEGIN\n    EXECUTE statement;\nEND\n$$;\n",
            ),
            (
                "src/dynamic.rs",
                "fn read(sql: &str) {\n    sqlx::query(sql);\n}\n",
            ),
        ]);
        let problems = fixture
            .findings()
            .into_iter()
            .map(|finding| (finding.path.display().to_string(), finding.problem))
            .collect::<Vec<_>>();
        assert_eq!(
            problems,
            [
                ("migrations/0004_dynamic.sql".into(), SqlProblem::DynamicSql),
                ("src/dynamic.rs".into(), SqlProblem::DynamicSql),
                (
                    "src/store/path.sql".into(),
                    SqlProblem::SearchPath {
                        schema: "tasks".into()
                    }
                ),
            ]
        );
    }

    #[test]
    fn sql_the_parser_refuses_fails_closed() {
        let fixture = Fixture::new(&[
            ("src/store/broken.sql", "SELECT FROM WHERE tasks.task\n"),
            (
                "migrations/0005_c.sql",
                "CREATE FUNCTION media.native() RETURNS int LANGUAGE c AS 'media', 'native';\n",
            ),
        ]);
        let findings = fixture.findings();
        assert_eq!(findings.len(), 2, "{findings:?}");
        assert!(matches!(
            &findings[0].problem,
            SqlProblem::UncheckedLanguage { language } if language == "c"
        ));
        assert_eq!(findings[1].path, Path::new("src/store/broken.sql"));
        assert!(matches!(findings[1].problem, SqlProblem::Unparsed { .. }));
    }

    #[test]
    fn offline_metadata_and_build_output_are_ignored() {
        let fixture = Fixture::new(&[
            (
                ".sqlx/query-0.json",
                r#"{"query": "SELECT * FROM tasks.task"}"#,
            ),
            (".sqlx/core.sql", "SELECT * FROM tasks.task\n"),
            ("target/debug/build.sql", "SELECT * FROM kernel.instance\n"),
            ("src/store/own.sql", "SELECT 1 FROM media.prediction\n"),
        ]);
        assert_eq!(fixture.findings(), Vec::new());
    }

    #[test]
    #[should_panic(expected = "only through veoveo-sdk-rust and core's runtime API")]
    fn the_assertion_names_the_way_to_core_state() {
        let fixture = Fixture::new(&[("src/store/by_task.sql", "SELECT * FROM tasks.task\n")]);
        assert_sql_confined(&fixture.0, "media");
    }
}
