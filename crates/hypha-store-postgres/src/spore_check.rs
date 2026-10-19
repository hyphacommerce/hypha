//! What a spore's migration may create (design note 0030, §5, SP4): objects named
//! `spore_<name>_*` only, foreign keys out of its tables that never block a core delete
//! (`ON DELETE CASCADE` or `SET NULL`), and nothing on a table outside its prefix: no
//! trigger, rule, policy, index, constraint or column there, and nothing of core's
//! dropped or changed in place (a column's type, nullability or default, a constraint's
//! or index's definition, a function's body: each object carries a fingerprint of its
//! definition).
//!
//! The runner reads the schema's catalog before and after each new migration of a
//! spore's set, in the migration's transaction, and compares: a problem rolls the
//! migration back and stops startup (and `setup`) naming the object. That makes "own
//! tables only" mechanical for the DDL; writes to core tables with raw SQL can't be told
//! apart this way (Q6).
//!
//! Only the current schema (`search_path`'s first) is read: that's where migrations
//! create what they create, and other schemas may be other databases' (tests run in a
//! schema each, at once).

use std::collections::BTreeSet;

use tokio_postgres::Transaction;

/// One catalog entry, as compared before and after.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CatalogObject {
    /// `relation`, `type`, `function`, `trigger`, `rule`, `policy`, `constraint`,
    /// `column`.
    pub kind: String,
    pub name: String,
    /// The table it's on (an index, trigger, rule, policy, constraint or column).
    pub table: Option<String>,
    /// A relation's `relkind`; a constraint's `contype:referenced table:confdeltype`;
    /// a function's arguments.
    pub detail: Option<String>,
    /// An MD5 of its whole definition as Postgres prints it (a column's type,
    /// nullability, default, identity and generation; a function's, constraint's,
    /// index's, trigger's, rule's, policy's or view's definition; an enum's labels; a
    /// table's persistence and row security), so a change in place shows.
    pub definition: Option<String>,
}

const SNAPSHOT: &str = "
WITH ns AS (SELECT oid FROM pg_namespace WHERE nspname = current_schema())
SELECT 'relation' AS kind, c.relname::text AS name, t.relname::text AS on_table,
       c.relkind::text AS detail,
       md5(c.relpersistence::text || c.relrowsecurity::text || c.relforcerowsecurity::text
           || coalesce(CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid) END, '')
           || coalesce(CASE WHEN c.relkind IN ('i', 'I') THEN pg_get_indexdef(c.oid) END, ''))
         AS definition
  FROM pg_class c
  LEFT JOIN pg_index i ON i.indexrelid = c.oid
  LEFT JOIN pg_class t ON t.oid = i.indrelid
 WHERE c.relnamespace = (SELECT oid FROM ns)
   AND c.relkind IN ('r', 'i', 'S', 'v', 'm', 'c', 'p', 'I', 'f')
UNION ALL
SELECT 'type', ty.typname::text, NULL, ty.typtype::text,
       md5(coalesce((SELECT string_agg(e.enumlabel, ',' ORDER BY e.enumsortorder)
                       FROM pg_enum e WHERE e.enumtypid = ty.oid), '')
           || coalesce(pg_get_constraintdef((SELECT oid FROM pg_constraint
                                              WHERE contypid = ty.oid LIMIT 1)), '')
           || format_type(ty.typbasetype, ty.typtypmod) || ty.typnotnull::text)
  FROM pg_type ty
 WHERE ty.typnamespace = (SELECT oid FROM ns) AND ty.typrelid = 0 AND ty.typelem = 0
UNION ALL
SELECT 'function', p.proname::text, NULL, pg_get_function_identity_arguments(p.oid),
       md5(CASE WHEN p.prokind IN ('f', 'p') THEN pg_get_functiondef(p.oid)
                ELSE p.prosrc END)
  FROM pg_proc p
 WHERE p.pronamespace = (SELECT oid FROM ns)
UNION ALL
SELECT 'trigger', tg.tgname::text, c.relname::text, NULL,
       md5(pg_get_triggerdef(tg.oid) || tg.tgenabled::text)
  FROM pg_trigger tg JOIN pg_class c ON c.oid = tg.tgrelid
 WHERE NOT tg.tgisinternal AND c.relnamespace = (SELECT oid FROM ns)
UNION ALL
SELECT 'rule', r.rulename::text, c.relname::text, NULL, md5(pg_get_ruledef(r.oid))
  FROM pg_rewrite r JOIN pg_class c ON c.oid = r.ev_class
 WHERE r.rulename <> '_RETURN' AND c.relnamespace = (SELECT oid FROM ns)
UNION ALL
SELECT 'policy', po.polname::text, c.relname::text, NULL,
       md5(po.polcmd::text || po.polpermissive::text || po.polroles::text
           || coalesce(pg_get_expr(po.polqual, po.polrelid), '')
           || coalesce(pg_get_expr(po.polwithcheck, po.polrelid), ''))
  FROM pg_policy po JOIN pg_class c ON c.oid = po.polrelid
 WHERE c.relnamespace = (SELECT oid FROM ns)
UNION ALL
SELECT 'constraint', co.conname::text, c.relname::text,
       co.contype::text || ':' || coalesce(f.relname::text, '') || ':' || co.confdeltype::text,
       md5(pg_get_constraintdef(co.oid) || co.convalidated::text)
  FROM pg_constraint co
  JOIN pg_class c ON c.oid = co.conrelid
  LEFT JOIN pg_class f ON f.oid = co.confrelid
 WHERE c.relnamespace = (SELECT oid FROM ns)
UNION ALL
SELECT 'column', a.attname::text, c.relname::text, NULL,
       md5(format_type(a.atttypid, a.atttypmod) || a.attnotnull::text
           || coalesce(pg_get_expr(d.adbin, d.adrelid), '')
           || a.attidentity::text || a.attgenerated::text || a.attcollation::text)
  FROM pg_attribute a JOIN pg_class c ON c.oid = a.attrelid
  LEFT JOIN pg_attrdef d ON d.adrelid = a.attrelid AND d.adnum = a.attnum
 WHERE a.attnum > 0 AND NOT a.attisdropped AND c.relkind IN ('r', 'p')
   AND c.relnamespace = (SELECT oid FROM ns)";

/// The current schema's catalog, as `CatalogObject`s.
pub async fn snapshot(
    tx: &Transaction<'_>,
) -> Result<BTreeSet<CatalogObject>, tokio_postgres::Error> {
    let rows = tx.query_typed(SNAPSHOT, &[]).await?;
    rows.iter()
        .map(|row| {
            Ok(CatalogObject {
                kind: row.try_get("kind")?,
                name: row.try_get("name")?,
                table: row.try_get("on_table")?,
                detail: row.try_get("detail")?,
                definition: row.try_get("definition")?,
            })
        })
        .collect()
}

/// Whose a name is: `module`'s (`spore_test` owns `spore_test_*`) unless a longer
/// module of `modules` owns it (`spore_test_x_*` is `spore_test_x`'s).
fn owned(name: &str, module: &str, modules: &[&str]) -> bool {
    let prefix = format!("{module}_");
    name.starts_with(&prefix)
        && !modules.iter().any(|other| {
            other.len() > module.len() && other.starts_with(&prefix) && {
                let theirs = format!("{other}_");
                name.starts_with(&theirs)
            }
        })
}

/// What `kind` (with its `relkind`) is called in a problem.
fn what(object: &CatalogObject) -> &str {
    match (object.kind.as_str(), object.detail.as_deref()) {
        ("relation", Some("r" | "p")) => "table",
        ("relation", Some("i" | "I")) => "index",
        ("relation", Some("S")) => "sequence",
        ("relation", Some("v")) => "view",
        ("relation", Some("m")) => "materialized view",
        ("relation", Some("c")) => "type",
        ("relation", Some("f")) => "foreign table",
        (kind, _) => kind,
    }
}

/// What an object is, apart from its definition: its kind, name and table, and a
/// function's arguments (overloads share a name).
fn identity(object: &CatalogObject) -> (&str, &str, Option<&str>, Option<&str>) {
    let arguments = match object.kind.as_str() {
        "function" => object.detail.as_deref(),
        _ => None,
    };
    (
        object.kind.as_str(),
        object.name.as_str(),
        object.table.as_deref(),
        arguments,
    )
}

/// Every rule `module`'s migration broke, from the catalog `before` and `after` it,
/// one line per object; empty when it kept to its own (`modules`: every module of the
/// run, so one spore's prefix isn't mistaken for another's).
pub fn problems(
    module: &str,
    modules: &[&str],
    before: &BTreeSet<CatalogObject>,
    after: &BTreeSet<CatalogObject>,
) -> Vec<String> {
    let mine = |name: &str| owned(name, module, modules);
    // Whether an object is core's (or another module's): on a table that isn't the
    // spore's, or, standing alone, named outside its prefix.
    let foreign = |object: &CatalogObject| match object.table.as_deref() {
        Some(table) => !mine(table),
        None => !mine(&object.name),
    };
    let existed =
        |object: &CatalogObject| before.iter().any(|old| identity(old) == identity(object));
    let prefix = format!("{module}_*");
    let mut problems = Vec::new();
    // Changed in place: the same object, another definition. Only the spore's own may.
    for old in before.iter().filter(|o| foreign(o)) {
        let changed = after
            .iter()
            .any(|new| identity(new) == identity(old) && new != old)
            && !after.contains(old);
        if changed {
            let kind = what(old);
            let name = &old.name;
            problems.push(match old.table.as_deref() {
                Some(table) => {
                    format!("{kind} {name:?} on {table:?} was changed, and it isn't the spore's")
                }
                None => format!("{kind} {name:?} was changed, and it isn't the spore's"),
            });
        }
    }
    for object in after.difference(before) {
        // A foreign object changed in place is reported above.
        if foreign(object) && existed(object) {
            continue;
        }
        let name = &object.name;
        let on = object.table.as_deref();
        let kind = what(object);
        match object.kind.as_str() {
            "relation" if !mine(name) => {
                problems.push(format!("{kind} {name:?} is not named {prefix}"));
            }
            "relation" => {
                if let Some(table) = on.filter(|t| !mine(t)) {
                    problems.push(format!(
                        "{kind} {name:?} is on {table:?}, a table that isn't the spore's"
                    ));
                }
            }
            "type" | "function" if !mine(name) => {
                problems.push(format!("{kind} {name:?} is not named {prefix}"));
            }
            "trigger" | "rule" | "policy" | "constraint" | "column" => {
                match on {
                    Some(table) if !mine(table) => problems.push(format!(
                        "{kind} {name:?} was added to {table:?}, a table that isn't the spore's"
                    )),
                    _ if object.kind == "constraint" => {
                        // `f:<referenced table>:<on delete>`: a foreign key out of the
                        // spore's tables may not block a delete outside them.
                        let detail = object.detail.as_deref().unwrap_or_default();
                        let mut parts = detail.splitn(3, ':');
                        if let (Some("f"), Some(target), Some(action)) =
                            (parts.next(), parts.next(), parts.next())
                            && !mine(target)
                            && !matches!(action, "c" | "n")
                        {
                            problems.push(format!(
                                "foreign key {name:?} on {:?} references {target:?} without \
                                 ON DELETE CASCADE or SET NULL",
                                on.unwrap_or_default()
                            ));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for object in before.difference(after) {
        let gone_from_core = match object.table.as_deref() {
            Some(table) => !mine(table),
            None => !mine(&object.name),
        };
        // A changed constraint or function shows as removed and added: only what's
        // really gone is reported, by name.
        let still_there = after
            .iter()
            .any(|o| o.kind == object.kind && o.name == object.name && o.table == object.table);
        if gone_from_core && !still_there {
            let kind = what(object);
            let name = &object.name;
            match object.table.as_deref() {
                Some(table) => problems.push(format!(
                    "{kind} {name:?} was dropped from {table:?}, a table that isn't the spore's"
                )),
                None => problems.push(format!(
                    "{kind} {name:?} was dropped, and it isn't the spore's"
                )),
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(kind: &str, name: &str, table: Option<&str>, detail: Option<&str>) -> CatalogObject {
        CatalogObject {
            kind: kind.to_string(),
            name: name.to_string(),
            table: table.map(str::to_string),
            detail: detail.map(str::to_string),
            definition: None,
        }
    }

    /// `object` with the definition fingerprint `definition`.
    fn defined(object: CatalogObject, definition: &str) -> CatalogObject {
        CatalogObject {
            definition: Some(definition.to_string()),
            ..object
        }
    }

    #[test]
    fn a_core_object_changed_in_place_is_refused_and_the_spores_own_may_change() {
        let modules = ["core", "spore_test"];
        let before = BTreeSet::from([
            defined(
                object("column", "title", Some("products"), None),
                "text not null",
            ),
            defined(object("function", "touch", None, Some("")), "v1"),
            defined(
                object("column", "body", Some("spore_test_notes"), None),
                "text",
            ),
        ]);
        let after = BTreeSet::from([
            defined(
                object("column", "title", Some("products"), None),
                "text null",
            ),
            defined(object("function", "touch", None, Some("")), "v2"),
            defined(
                object("column", "body", Some("spore_test_notes"), None),
                "varchar",
            ),
        ]);
        assert_eq!(
            problems("spore_test", &modules, &before, &after),
            [
                "column \"title\" on \"products\" was changed, and it isn't the spore's",
                "function \"touch\" was changed, and it isn't the spore's",
            ]
        );
    }

    #[test]
    fn a_new_overload_of_a_core_function_is_refused_as_a_new_function() {
        let before = BTreeSet::from([defined(
            object("function", "touch", None, Some("integer")),
            "v1",
        )]);
        let mut after = before.clone();
        after.insert(defined(
            object("function", "touch", None, Some("text")),
            "v1",
        ));
        assert_eq!(
            problems("spore_test", &["core", "spore_test"], &before, &after),
            ["function \"touch\" is not named spore_test_*"]
        );
    }

    fn core() -> BTreeSet<CatalogObject> {
        BTreeSet::from([
            object("relation", "products", None, Some("r")),
            object("column", "id", Some("products"), None),
        ])
    }

    fn after(added: &[CatalogObject]) -> BTreeSet<CatalogObject> {
        let mut set = core();
        set.extend(added.iter().cloned());
        set
    }

    #[test]
    fn a_name_belongs_to_the_longest_module_it_starts_with() {
        let modules = ["spore_test", "spore_test_x"];
        assert!(owned("spore_test_notes", "spore_test", &modules));
        assert!(!owned("spore_test_x_notes", "spore_test", &modules));
        assert!(owned("spore_test_x_notes", "spore_test_x", &modules));
        assert!(!owned("spore_testing", "spore_test", &modules));
        assert!(!owned("products", "spore_test", &modules));
    }

    #[test]
    fn its_own_tables_indexes_and_cascading_keys_pass() {
        let added = [
            object("relation", "spore_test_notes", None, Some("r")),
            object(
                "relation",
                "spore_test_notes_pkey",
                Some("spore_test_notes"),
                Some("i"),
            ),
            object("column", "product_id", Some("spore_test_notes"), None),
            object(
                "constraint",
                "notes_product",
                Some("spore_test_notes"),
                Some("f:products:c"),
            ),
            object("trigger", "touch", Some("spore_test_notes"), None),
            object("function", "spore_test_touch", None, Some("")),
        ];
        assert!(
            problems(
                "spore_test",
                &["core", "spore_test"],
                &core(),
                &after(&added)
            )
            .is_empty()
        );
    }

    #[test]
    fn every_misstep_is_named() {
        let added = [
            object("relation", "notes", None, Some("r")),
            object(
                "relation",
                "spore_test_by_title",
                Some("products"),
                Some("i"),
            ),
            object("column", "rating", Some("products"), None),
            object("trigger", "spore_test_audit", Some("products"), None),
            object(
                "constraint",
                "notes_product",
                Some("spore_test_notes"),
                Some("f:products:a"),
            ),
            object("type", "mood", None, Some("e")),
        ];
        assert_eq!(
            problems(
                "spore_test",
                &["core", "spore_test"],
                &core(),
                &after(&added)
            ),
            [
                "column \"rating\" was added to \"products\", a table that isn't the spore's",
                "foreign key \"notes_product\" on \"spore_test_notes\" references \"products\" without ON DELETE CASCADE or SET NULL",
                "table \"notes\" is not named spore_test_*",
                "index \"spore_test_by_title\" is on \"products\", a table that isn't the spore's",
                "trigger \"spore_test_audit\" was added to \"products\", a table that isn't the spore's",
                "type \"mood\" is not named spore_test_*",
            ]
        );
    }

    #[test]
    fn dropping_cores_is_named() {
        let mut later = core();
        later.retain(|o| o.kind != "column");
        assert_eq!(
            problems("spore_test", &["spore_test"], &core(), &later),
            ["column \"id\" was dropped from \"products\", a table that isn't the spore's"]
        );
    }
}
