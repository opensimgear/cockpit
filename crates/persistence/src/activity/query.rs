use super::*;
use rusqlite::{params_from_iter, types::Value};

// Observation-cache writes are internal bookkeeping, even when an older writer
// stored them. Keep the records intact but exclude them from activity and facets.
const VISIBLE: &str = "NOT (event_type='ui_preferences_save' AND coalesce(entity_id,'')='remembered' AND level='info')";

fn conditions(query: &ActivityQuery) -> Result<(String, Vec<Value>), String> {
    if query.limit == 0
        || query.limit > 500
        || query.offset > i64::MAX as usize
        || query.since.is_some_and(|v| !valid_date(v))
        || query.until.is_some_and(|v| !valid_date(v))
        || matches!((query.since, query.until), (Some(a), Some(b)) if a > b)
        || query.search.len() > 4096
        || query
            .through_id
            .is_some_and(|id| id == 0 || id > 9_007_199_254_740_991)
    {
        return Err("Invalid activity query dates, search, or pagination.".into());
    }
    let mut clauses = vec![VISIBLE.to_owned()];
    let mut values = Vec::new();
    for (column, filters) in [
        ("category", &query.categories),
        ("level", &query.levels),
        ("event_type", &query.event_types),
    ] {
        if filters.len() > 100 || filters.iter().any(|v| v.len() > 4096) {
            return Err("Invalid activity filters.".into());
        }
        if !filters.is_empty() {
            clauses.push(format!(
                "{column} IN ({})",
                vec!["?"; filters.len()].join(",")
            ));
            values.extend(filters.iter().cloned().map(Value::Text));
        }
    }
    if let Some(entity) = &query.entity_id {
        if entity.len() > 4096 {
            return Err("Invalid activity entity filter.".into());
        }
        clauses.push("entity_id=?".into());
        values.push(Value::Text(entity.clone()));
    }
    for (column, value) in [
        ("occurred_at>=?", query.since),
        ("occurred_at<=?", query.until),
    ] {
        if let Some(value) = value {
            clauses.push(column.into());
            values.push(Value::Integer(value));
        }
    }
    // Treat every user token literally. FTS operators, quotes and punctuation
    // cannot become executable MATCH syntax; unicode61 tokenizes stored text.
    if !query.search.trim().is_empty() {
        let terms: Vec<_> = query
            .search
            .split_whitespace()
            .filter(|s| s.chars().any(char::is_alphanumeric))
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect();
        if terms.is_empty() {
            clauses.push("0".into());
        } else {
            clauses
                .push("id IN (SELECT rowid FROM activity_fts WHERE activity_fts MATCH ?)".into());
            values.push(Value::Text(terms.join(" AND ")));
        }
    }
    Ok((
        if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        },
        values,
    ))
}

fn facet(connection: &Connection, column: &str) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(&format!(
            "SELECT DISTINCT {column} FROM activity_events WHERE {VISIBLE} ORDER BY {column}"
        ))
        .map_err(failure)?;
    let values = statement
        .query_map([], |row| row.get(0))
        .map_err(failure)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(failure)?;
    Ok(values)
}

impl Database {
    pub fn activity_query(&self, query: &ActivityQuery) -> Result<ActivityPage, String> {
        let (conditions, mut values) = conditions(query)?;
        let mut connection = self.connection()?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(failure)?;
        self.verify_epoch(&tx)?;
        prune(&tx)?;
        // Freeze membership by insertion identity, including backdated events:
        // later writes cannot shift OFFSET pages within this browsing session.
        let through_id = match query.through_id {
            Some(id) => id,
            None => tx
                .query_row(
                    "SELECT coalesce(max(id),0) FROM activity_events",
                    [],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(failure)? as u64,
        };
        let conditions = format!(
            "{conditions}{}id<=?",
            if conditions.is_empty() {
                " WHERE "
            } else {
                " AND "
            }
        );
        values.push(Value::Integer(through_id as i64));
        let total: i64 = tx
            .query_row(
                &format!("SELECT count(*) FROM activity_events{conditions}"),
                params_from_iter(&values),
                |row| row.get(0),
            )
            .map_err(failure)?;
        values.extend([
            Value::Integer(query.limit as i64),
            Value::Integer(query.offset as i64),
        ]);
        let events = {
            let mut statement = tx.prepare(&format!("SELECT id,occurred_at,category,event_type,level,message,detail,entity_id,entity_name,source FROM activity_events{conditions} ORDER BY occurred_at DESC,id DESC LIMIT ? OFFSET ?")).map_err(failure)?;
            let events = statement
                .query_map(params_from_iter(&values), |row| {
                    Ok(ActivityEvent {
                        id: row.get::<_, i64>(0)? as u64,
                        occurred_at: row.get(1)?,
                        category: row.get(2)?,
                        event_type: row.get(3)?,
                        level: row.get(4)?,
                        message: row.get(5)?,
                        detail: row.get(6)?,
                        entity_id: row.get(7)?,
                        entity_name: row.get(8)?,
                        source: row.get(9)?,
                    })
                })
                .map_err(failure)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(failure)?;
            events
        };
        let page = ActivityPage {
            events,
            through_id,
            total: total as usize,
            categories: facet(&tx, "category")?,
            levels: facet(&tx, "level")?,
            event_types: facet(&tx, "event_type")?,
        };
        tx.commit().map_err(failure)?;
        Ok(page)
    }
}
