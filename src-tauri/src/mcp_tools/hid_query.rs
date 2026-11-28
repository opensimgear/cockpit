//! Stateless bounded views of the same detail/current/report sources as the inspector.
use super::*;
#[path = "hid_query_model.rs"]
mod model;

const PAGE_BYTES: usize = 200 * 1024;
const MAX_NODES: usize = 50_000;

pub(super) fn handles(name: &str) -> bool {
    matches!(name, "controller_hid_descriptor_query" | "controller_hid_monitor_query")
}
fn text_schema(max: usize) -> Value {
    json!({"type":"string","minLength":1,"maxLength":max})
}
fn count(max: usize) -> Value {
    json!({"type":"integer","minimum":0,"maximum":max})
}
fn cursor() -> Value {
    json!({"type":"string","minLength":1,"maxLength":20,"pattern":"^(0|[1-9][0-9]*)$"})
}
fn object(properties: Vec<(&str, Value)>, required: &[&str]) -> Value {
    json!({"type":"object","additionalProperties":false,"properties":properties.into_iter().map(|(k,v)|(k.to_owned(),v)).collect::<serde_json::Map<_,_>>(),"required":required})
}
fn tool(name: &str, description: &str, properties: Vec<(&str, Value)>) -> Value {
    json!({"name":name,"description":description,"inputSchema":object(properties,&["source"]),"outputSchema":definitions::result_schema(),"annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
}
pub(super) fn catalogue() -> Vec<Value> {
    let source = || {
        vec![
            ("source", json!({"type":"string","enum":["live","snapshot","replay"]})),
            ("interfaceId", text_schema(128)),
            ("replayId", text_schema(128)),
        ]
    };
    let mut descriptor = source();
    descriptor.extend([
        ("search", text_schema(256)),
        ("offset", count(MAX_NODES)),
        ("queryFingerprint", text_schema(80)),
        ("limit", json!({"type":"integer","minimum":1,"maximum":128})),
    ]);
    let mut monitor = source();
    monitor[0].1 = json!({"type":"string","enum":["live","replay"]});
    monitor.extend([("generation",text_schema(128)),("after",cursor()),("elapsedUs",cursor()),("seek",json!({"type":"boolean"})),
        ("kind",json!({"type":"string","enum":["input","output","feature"]})),("report",text_schema(3)),("collection",text_schema(256)),("usage",text_schema(256)),("fieldId",text_schema(256)),("elementIndex",count(65535)),
        ("changed",json!({"type":"boolean"})),("active",json!({"type":"boolean"})),("previous",json!({"type":"array","maxItems":16384,"items":object(vec![("fieldId",text_schema(256)),("elementIndex",count(65535)),("sample",json!({"type":"object"}))],&["fieldId","elementIndex","sample"])})),
        ("valueOffset",count(20_000)),("reportOffset",count(256)),("queryFingerprint",text_schema(80)),("limit",json!({"type":"integer","minimum":1,"maximum":128}))]);
    vec![
        tool("controller_hid_descriptor_query","Search the native descriptor/capability tree by case-insensitive path and exact value. source live inspects metadata without opening capture; snapshot reads cached evidence; replay reads the authenticated archive descriptor. Paginate matching nodes with nodes.nextOffset and the returned queryFingerprint; search scans at most 50000 nodes and depth 64. Values longer than 4096 characters are explicitly previewed; use inspection_slice for exact evidence.",descriptor),
        tool("controller_hid_monitor_query","Filter the inspector's native current absolute samples plus the latest 128 received event/relative samples, and raw reports. source live requires interfaceId/generation; replay requires replayId. kind/report/collection/usage match UI filters; fieldId and elementIndex match exact decoded identities. changed requires previous samples supplied by the caller; absent previous identities count as changed. active selects valid nonzero buttons only. Raw context is filtered by report only and retains gap/terminal markers. limit is shared across decoded and raw entries. Continue using valueOffset=values.endOffset and reportOffset=reports.endOffset with the same source cursor and queryFingerprint until both nextOffset fields are null, then advance sourceNext. Live observations may advance between calls; query_changed requires restarting offset zero. Oversized entries are explicit stubs, never silently skipped.",monitor),
    ]
}
fn invalid(message: &str) -> Value {
    failure("invalid_arguments", message, false)
}
fn canonical(args: &Value, key: &str) -> Result<(), Value> {
    if let Some(value) = args.get(key) {
        if !value
            .as_str()
            .is_some_and(|s| s.parse::<u64>().ok().is_some_and(|n| n.to_string() == s))
        {
            return Err(invalid(&format!("{key} must be a canonical unsigned 64-bit decimal.")));
        }
    }
    Ok(())
}
fn validate_source(args: &Value, monitor: bool) -> Result<(), Value> {
    let replay = args["source"] == "replay";
    for key in if replay {
        vec!["replayId"]
    } else if monitor {
        vec!["interfaceId", "generation"]
    } else {
        vec!["interfaceId"]
    } {
        text(args, key)?;
    }
    let forbidden = if replay {
        vec!["interfaceId", "generation"]
    } else {
        vec!["replayId", "elapsedUs", "seek"]
    };
    if forbidden.iter().any(|key| args.get(key).is_some()) {
        return Err(invalid(
            "Source identity and replay clock arguments must match the selected source.",
        ));
    }
    if args["seek"] == true && (args.get("elapsedUs").is_none() || args["after"].as_str().is_some_and(|s| s != "0")) {
        return Err(invalid("Replay seek requires elapsedUs and after omitted or 0."));
    }
    if args["changed"] == true && args.get("previous").is_none() {
        return Err(invalid(
            "changed requires an explicit previous sample array (empty is an initial baseline).",
        ));
    }
    if args
        .get("previous")
        .is_some_and(|value| value.to_string().len() > MAX_REQUEST / 2)
    {
        return Err(invalid("previous exceeds half the native request budget. Query a narrower field/report subset before supplying its baseline."));
    }
    canonical(args, "after")?;
    canonical(args, "elapsedUs")?;
    if let Some(report) = args["report"].as_str() {
        if !report.parse::<u8>().ok().is_some_and(|n| n.to_string() == report) {
            return Err(invalid("report must be a canonical report ID from 0 through 255."));
        }
    }
    Ok(())
}
fn oversized(value: &Value) -> Value {
    let identity = value.get("value").filter(|v| v.is_object()).unwrap_or(value);
    json!({"unavailable":"query_entry_exceeds_byte_budget","encodedBytes":value.to_string().len(),"fieldId":identity["fieldId"],"elementIndex":identity["elementIndex"],"sequence":identity["sequence"],"receivedUs":identity["receivedUs"],"kind":identity["kind"],"evidence":"Use controller_hid_inspection_slice for the exact native source."})
}
fn fence(args: &Value, fingerprint: &str) -> Result<(), Value> {
    let paging = ["offset", "valueOffset", "reportOffset"]
        .iter()
        .any(|key| args[key].as_u64().is_some_and(|n| n > 0));
    if paging && args.get("queryFingerprint").is_none() {
        return Err(invalid("Paginating requires the previous response queryFingerprint."));
    }
    if args["queryFingerprint"]
        .as_str()
        .is_some_and(|expected| expected != fingerprint)
    {
        return Err(failure(
            "query_changed",
            "The native query window changed. Restart at offset zero and inspect its evidence.",
            true,
        ));
    }
    Ok(())
}
fn signature(args: &Value) -> Value {
    let mut value = args.clone();
    if let Some(map) = value.as_object_mut() {
        for key in ["offset", "valueOffset", "reportOffset", "limit", "queryFingerprint"] {
            map.remove(key);
        }
    }
    value
}
fn fingerprint(value: &impl serde::Serialize) -> String {
    struct Writer(Sha256);
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer(Sha256::new());
    serde_json::to_writer(&mut writer, value).expect("JSON values serialize");
    format!("sha256:{:x}", writer.0.finalize())
}
fn bounded(value: Value, budget: usize) -> Value {
    if value.to_string().len() > budget {
        oversized(&value)
    } else {
        value
    }
}
fn page(values: &[Value], offset: usize, limit: usize) -> Value {
    projected_page(values, offset, limit, Clone::clone)
}
fn projected_page<T>(values: &[T], offset: usize, limit: usize, project: impl Fn(&T) -> Value) -> Value {
    let mut entries = vec![];
    let mut next = offset.min(values.len());
    let mut bytes = 0;
    for value in values.iter().skip(offset).take(limit) {
        let entry = bounded(project(value), PAGE_BYTES);
        let size = entry.to_string().len();
        if !entries.is_empty() && bytes + size > PAGE_BYTES {
            break;
        }
        bytes += size;
        entries.push(entry);
        next += 1;
    }
    json!({"entries":entries,"offset":offset,"endOffset":next,"nextOffset":if next<values.len(){Some(next)}else{None},"total":values.len()})
}
struct Tree<'a> {
    query: &'a str,
    nodes: Vec<Value>,
    visited: usize,
    limited: bool,
    offset: usize,
    limit: usize,
    matched: usize,
    bytes: usize,
    page_full: bool,
}
impl Tree<'_> {
    fn node(&mut self, line: String, path: &str, depth: usize, path_truncated: bool) {
        if self.query.is_empty()
            || path.to_lowercase().contains(self.query)
            || line.to_lowercase().contains(self.query)
            || format!(
                "{path} {}",
                line.chars().take(self.query.chars().count()).collect::<String>()
            )
            .to_lowercase()
            .contains(self.query)
        {
            let ordinal = self.matched;
            self.matched += 1;
            if ordinal < self.offset || self.page_full {
                return;
            }
            let preview = line.chars().take(4096).collect::<String>();
            let node = json!({"path":path,"name":path.rsplit('.').next().unwrap_or(path),"pathTruncated":path_truncated,"value":preview,"depth":depth,"valueTruncated":preview.len()!=line.len(),"valueBytes":line.len()});
            let size = node.to_string().len();
            if self.nodes.len() >= self.limit || (!self.nodes.is_empty() && self.bytes + size > PAGE_BYTES) {
                self.page_full = true;
                return;
            }
            self.bytes += size;
            self.nodes.push(node);
        }
    }
    fn child(&mut self, value: &Value, path: &str, key: &str, depth: usize) {
        if path.len() + 1 + key.len() > 4096 {
            self.limited = true;
            self.visited += 1;
            let mut label = path.chars().take(1023).collect::<String>();
            label.push('.');
            label.extend(key.chars().take((4096 - label.len()) / 4));
            self.node(
                "Subtree exceeds the bounded path budget; inspect the exact native source.".into(),
                &label,
                depth,
                true,
            );
            return;
        }
        self.walk(value, format!("{path}.{key}"), depth);
    }
    fn walk(&mut self, value: &Value, path: String, depth: usize) {
        if depth > 64 || self.visited >= MAX_NODES {
            self.limited = true;
            return;
        }
        self.visited += 1;
        let line = match value {
            Value::Array(a) => format!("{} entries", a.len()),
            Value::Object(o) => format!("{} properties", o.len()),
            _ => model::exact(value),
        };
        self.node(line, &path, depth, false);
        match value {
            Value::Array(a) => {
                for (index, child) in a.iter().enumerate() {
                    self.child(child, &path, &index.to_string(), depth + 1);
                    if self.visited >= MAX_NODES {
                        self.limited = true;
                        break;
                    }
                }
            }
            Value::Object(o) => {
                for (key, child) in o {
                    self.child(child, &path, key, depth + 1);
                    if self.visited >= MAX_NODES {
                        self.limited = true;
                        break;
                    }
                }
            }
            _ => {}
        }
    }
}
impl Tools {
    fn hid_query_detail(&self, args: &Value, client: &str) -> Result<Value, Value> {
        if args["source"] == "replay" {
            let response = self.native(
                "controller_hid_replay_read",
                json!({"replayId":args["replayId"],"part":"descriptor"}),
                client,
                Some("controllers"),
            )?;
            Ok(response["descriptor"].clone())
        } else {
            self.native(
                if args["source"] == "snapshot" {
                    "controller_hid_snapshot"
                } else {
                    "controller_hid_inspect"
                },
                select(args, &["interfaceId"]),
                client,
                Some("controllers"),
            )
        }
    }
    pub(super) fn hid_query_tool(&self, name: &str, args: &Value, client: &str) -> Result<Value, Value> {
        let monitor = name == "controller_hid_monitor_query";
        validate_source(args, monitor)?;
        let descriptor = self.hid_query_detail(args, client)?;
        let limit = args["limit"].as_u64().unwrap_or(128) as usize;
        if !monitor {
            let fingerprint = fingerprint(&(
                model::detail(&descriptor),
                &descriptor["contractFingerprint"],
                &signature(args),
            ));
            fence(args, &fingerprint)?;
            let search = args["search"].as_str().unwrap_or("").trim().to_lowercase();
            let offset = args["offset"].as_u64().unwrap_or(0) as usize;
            let mut tree = Tree {
                query: &search,
                nodes: vec![],
                visited: 0,
                limited: false,
                offset,
                limit,
                matched: 0,
                bytes: 0,
                page_full: false,
            };
            tree.walk(model::detail(&descriptor), "interface".into(), 0);
            if offset > tree.matched {
                return Err(invalid("offset is beyond the matching descriptor node count."));
            }
            let end = offset + tree.nodes.len();
            let nodes = json!({"entries":tree.nodes,"offset":offset,"endOffset":end,"nextOffset":if end<tree.matched{Some(end)}else{None},"total":tree.matched});
            return Ok(
                json!({"source":args["source"],"interfaceId":args["interfaceId"],"replayId":args["replayId"],"queryFingerprint":fingerprint,"contractFingerprint":descriptor["contractFingerprint"],"access":model::detail(&descriptor)["access"],"nodes":nodes,"visited":tree.visited,"scanLimited":tree.limited,"traversalLimited":tree.limited,"declaredFields":model::field_count(&descriptor)}),
            );
        }
        let replay = args["source"] == "replay";
        let (mut state, mut evidence, reports, source_next) = if replay {
            let mut request = select(args, &["replayId", "after", "elapsedUs", "seek"]);
            request["limit"] = json!(128);
            let mut response = self.native("controller_hid_replay_read", request, client, Some("controllers"))?;
            let reports = response
                .as_object_mut()
                .and_then(|o| o.remove("records"))
                .unwrap_or(json!([]));
            let state = response
                .as_object_mut()
                .and_then(|o| o.remove("state"))
                .unwrap_or(Value::Null);
            let next = response["next"].clone();
            (state, response, reports, next)
        } else {
            let current = self.native(
                "controller_hid_state",
                select(args, &["interfaceId", "generation"]),
                client,
                Some("controllers"),
            )?;
            if descriptor.get("contractFingerprint").is_some()
                && descriptor["contractFingerprint"] != current["contractFingerprint"]
            {
                return Err(failure(
                    "stale_contract",
                    "The inspected contract differs from the active generation. Reinspect the selected interface.",
                    false,
                ));
            }
            let mut request = select(args, &["interfaceId", "generation", "after"]);
            request["limit"] = json!(128);
            let response = self.native("controller_hid_reports", request, client, Some("controllers"))?;
            let mut evidence = response["result"].clone();
            let reports = evidence
                .as_object_mut()
                .and_then(|o| o.remove("reports"))
                .unwrap_or(json!([]));
            let next = evidence.get("next").cloned().unwrap_or_else(|| {
                model::rows(&reports)
                    .last()
                    .map(|r| r["sequence"].clone())
                    .unwrap_or_else(|| args.get("after").cloned().unwrap_or(json!("0")))
            });
            (current["result"].clone(), evidence, reports, next)
        };
        let field_list = model::fields(&descriptor);
        let all_reports = model::rows(&reports);
        let mut values = model::rows(&state["values"]).to_vec();
        if let Some(o) = state.as_object_mut() {
            o.remove("values");
        }
        let events = model::report_values(all_reports)
            .into_iter()
            .filter(|v| !model::exact(&v["semantics"]).eq_ignore_ascii_case("absolute"))
            .collect::<Vec<_>>();
        values.extend(events.into_iter().rev().take(128).collect::<Vec<_>>().into_iter().rev());
        let fingerprint = fingerprint(&(
            &descriptor["contractFingerprint"],
            &state,
            &reports,
            &values,
            &signature(args),
        ));
        fence(args, &fingerprint)?;
        let previous = model::rows(&args["previous"]);
        let selected_values = values
            .iter()
            .filter(|value| {
                let field = model::find_field(&field_list, &value);
                model::matches(field, value, args, previous)
            })
            .collect::<Vec<_>>();
        let raw = all_reports
            .iter()
            .filter(|row| {
                args["report"].as_str().is_none_or(|id| {
                    model::exact(
                        row.get("reportId")
                            .filter(|value| !value.is_null())
                            .unwrap_or(&row["windowsReportId"]),
                    ) == id
                }) || matches!(row["kind"].as_str(), Some("gap" | "terminal"))
            })
            .cloned()
            .collect::<Vec<_>>();
        let value_offset = args["valueOffset"].as_u64().unwrap_or(0) as usize;
        let report_offset = args["reportOffset"].as_u64().unwrap_or(0) as usize;
        if value_offset > selected_values.len() || report_offset > raw.len() {
            return Err(invalid(
                "valueOffset or reportOffset is beyond its matching entry count.",
            ));
        }
        let value_page = projected_page(&selected_values, value_offset, limit, |value| {
            let field = model::find_field(&field_list, value);
            json!({"presentation":model::presentation(value,field),"field":field,"value":value})
        });
        let used = model::rows(&value_page["entries"]).len();
        let report_page = page(&raw, report_offset, limit - used);
        // Keep diagnostic and terminal evidence independently visible when lists filter to zero.
        evidence = bounded(evidence, 64 * 1024);
        state = bounded(state, 64 * 1024);
        Ok(
            json!({"source":args["source"],"interfaceId":args["interfaceId"],"generation":args["generation"],"replayId":args["replayId"],"queryFingerprint":fingerprint,"state":state,"evidence":evidence,"sourceNext":source_next,
            "values":value_page,"reports":report_page}),
        )
    }
}

#[cfg(test)]
#[path = "hid_query_tests.rs"]
mod tests;
