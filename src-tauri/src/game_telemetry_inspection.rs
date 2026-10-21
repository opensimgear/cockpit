//! Bounded, passive telemetry captures. Inspection never owns a profile or an output.
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(crate) struct Capture {
    pub status: String,
    pub source: String,
    pub context: Value,
    pub packet: Value,
    pub age_ms: Option<u64>,
    pub values: BTreeMap<String, Value>,
    pub ages: BTreeMap<String, Option<u64>>,
    pub stale_ids: BTreeSet<String>,
    pub message: Option<String>,
    pub at: Instant,
}
impl Capture {
    pub(crate) fn assetto(snapshot: &crate::game_telemetry::Snapshot) -> Self {
        let detail = snapshot.value();
        Self {
            status: if snapshot.fresh {
                "live".into()
            } else {
                snapshot.status.clone()
            },
            source: "shared-memory".into(),
            context: detail["context"].clone(),
            packet: json!(snapshot.packet_id),
            age_ms: snapshot.age_ms,
            values: snapshot.values(),
            ages: BTreeMap::new(),
            stale_ids: BTreeSet::new(),
            message: snapshot.error.clone(),
            at: Instant::now(),
        }
    }
    pub(crate) fn export(game: &str, snapshot: &crate::dcs_profile::ExportSnapshot) -> Self {
        Self {
            status: (if snapshot.fresh {
                "live"
            } else if snapshot.aircraft.is_some() && !snapshot.mission_active {
                "paused"
            } else if snapshot.age_ms.is_some() {
                "stale"
            } else {
                "waiting"
            })
            .into(),
            source: if game == "dcs" { "dcs-bios" } else { "rref" }.into(),
            context: json!({"aircraft":snapshot.aircraft,"epoch":snapshot.epoch}),
            packet: json!(snapshot.counter),
            age_ms: snapshot.age_ms,
            values: if snapshot.fresh {
                crate::game_channels::values(game, &snapshot.values)
            } else {
                BTreeMap::new()
            },
            ages: BTreeMap::new(),
            stale_ids: BTreeSet::new(),
            message: None,
            at: Instant::now(),
        }
    }
    pub(crate) fn expired(&self) -> bool {
        self.at.elapsed() > Duration::from_secs(2)
    }
}
fn bounded(value: &Value, fallback: &str, max: usize) -> String {
    value
        .as_str()
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .chars()
        .filter(|c| !c.is_control())
        .take(max)
        .collect()
}
pub(crate) fn channels(catalogue: &Value) -> Vec<Value> {
    let mut result = catalogue["channels"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for control in catalogue["controls"].as_array().into_iter().flatten() {
        let Some(id) = control["id"].as_str() else {
            continue;
        };
        for output in control["outputs"].as_array().into_iter().flatten() {
            result.push(json!({"id":format!("{id}:{}",output["index"]),"label":control["label"],"category":control["category"].as_str().unwrap_or("cockpit"),"group":control["category"].as_str().unwrap_or("cockpit"),"type":if output["type"] == "string" {"string"} else {"integer"},"unit":output["unit"],"nativeField":control["identifier"].as_str().unwrap_or(id),"description":control["description"],"sourceModule":control["sourceModule"]}));
        }
    }
    result.sort_by(|a, b| {
        let canonical = |v: &Value| v["canonical"] == true;
        canonical(b)
            .cmp(&canonical(a))
            .then_with(|| a["id"].as_str().cmp(&b["id"].as_str()))
    });
    result.dedup_by(|a, b| a["id"] == b["id"]);
    result
}
pub(crate) fn category(channel: &Value) -> String {
    bounded(
        channel
            .get("category")
            .or_else(|| channel.get("group"))
            .unwrap_or(&Value::Null),
        "telemetry",
        128,
    )
}
pub(crate) fn group_label(id: &str) -> String {
    let mut result = id.replace(['_', '-'], " ");
    if let Some(first) = result.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    result
}
pub(crate) fn page(
    all: &[Value],
    capture: Option<&Capture>,
    offset: usize,
    limit: usize,
    search: &str,
    group: &str,
) -> (Vec<Value>, usize, Vec<Value>, Vec<String>) {
    let mut groups = BTreeMap::<String, usize>::new();
    for channel in all {
        *groups.entry(category(channel)).or_default() += 1;
    }
    let groups = groups
        .into_iter()
        .map(|(id, total)| json!({"label":group_label(&id),"id":id,"total":total}))
        .collect();
    let filtered = all
        .iter()
        .filter(|channel| {
            (group.is_empty() || category(channel) == group)
                && (search.is_empty()
                    || ["id", "label", "nativeField", "description"]
                        .iter()
                        .any(|key| {
                            channel[key]
                                .as_str()
                                .is_some_and(|value| value.to_lowercase().contains(search))
                        }))
        })
        .collect::<Vec<_>>();
    let total = filtered.len();
    let mut requested = vec![];
    let rows = filtered.into_iter().skip(offset).take(limit).map(|channel| {
        let id = channel["id"].as_str().unwrap_or("");
        if channel["supported"] != false {
            requested.push(id.to_owned());
        }
        let value = capture.filter(|c|c.status == "live" && !c.expired()).and_then(|c|c.values.get(id)).filter(|v| v.is_number() || v.is_string() || v.is_boolean());
        let status = if channel["supported"] == false { "unavailable" } else if capture.is_some_and(|capture|capture.stale_ids.contains(id)){"stale"}else if value.is_some() { "known" } else if capture.is_some_and(|c|c.expired() || matches!(c.status.as_str(),"stale"|"paused")) { "stale" } else { "unknown" };
        let age = capture.and_then(|c|c.ages.get(id).copied().unwrap_or(c.age_ms).map(|age|age.saturating_add(c.at.elapsed().as_millis() as u64)));
        json!({"id":id,"label":bounded(&channel["label"],id,256),"category":category(channel),"group":channel["group"].as_str().unwrap_or("telemetry"),"type":channel["type"].as_str().unwrap_or("number"),"unit":channel["unit"].as_str(),"status":status,"value":if status == "known" {value.cloned().unwrap_or(Value::Null)} else {Value::Null},"ageMs":age,"nativeId":channel["nativeId"].as_str().unwrap_or(id),"nativeField":channel["nativeField"].as_str().or_else(||channel["dataref"].as_str()).unwrap_or(id),"description":channel["note"].as_str().or_else(||channel["description"].as_str()),"provider":capture.map(|c|c.source.as_str()).unwrap_or("")})
    }).collect();
    (rows, total, groups, requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paging_preserves_known_zero_and_never_replays_paused_samples() {
        let all = vec![
            json!({"id":"engine.rpm","label":"RPM","category":"engine","type":"number","unit":"rpm"}),
            json!({"id":"bad","label":"Unused","category":"physics","supported":false}),
        ];
        let mut capture = Capture {
            status: "live".into(),
            source: "fixture".into(),
            context: Value::Null,
            packet: json!(10),
            age_ms: Some(0),
            values: BTreeMap::from([("engine.rpm".into(), json!(0))]),
            ages: BTreeMap::new(),
            stale_ids: BTreeSet::new(),
            message: None,
            at: Instant::now(),
        };
        let (rows, total, groups, requested) = page(&all, Some(&capture), 0, 1, "rpm", "");
        assert_eq!(total, 1);
        assert_eq!(groups.len(), 2);
        assert_eq!(requested, ["engine.rpm"]);
        assert_eq!(rows[0]["status"], "known");
        assert_eq!(rows[0]["value"], 0);
        capture.status = "paused".into();
        assert_eq!(
            page(&all, Some(&capture), 0, 1, "", "").0[0]["value"],
            Value::Null
        );
        assert_eq!(
            page(&all, Some(&capture), 1, 1, "", "").0[0]["status"],
            "unavailable"
        );
    }

    #[test]
    fn xplane_mixed_and_unsupported_only_pages_preserve_rows_without_requesting_unavailable_datarefs() {
        let directory = crate::test_directory();
        let plugins = directory.path().join("Resources/plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::write(plugins.join("DataRefs.txt"), "2 1210\nsim/flightmodel/forces/g_nrml\tfloat\tn\tGs\tNormal acceleration\nsim/flightmodel/forces/oversized\tfloat[8193]\tn\tnewtons\tUnsupported array\nsim/flightmodel/forces/fside_total\tfloat\tn\tnewtons\tLateral force\n").unwrap();
        let catalogue = crate::xplane::catalogue_for_installation(&directory.path().join("X-Plane.exe")).unwrap();
        let all = channels(&catalogue).into_iter().filter(|channel| channel["id"].as_str().is_some_and(|id|id.starts_with("sim/"))).collect::<Vec<_>>();
        let supported = "sim/flightmodel/forces/g_nrml";
        let unsupported = "sim/flightmodel/forces/oversized";
        let capture = Capture {
            status: "live".into(), source: "rref".into(), context: json!({"aircraft":"Aircraft/Test.acf"}), packet: json!(10), age_ms: Some(0),
            values: BTreeMap::from([(supported.into(), json!(0)), (unsupported.into(), json!(999))]),
            ages: BTreeMap::new(), stale_ids: BTreeSet::new(), message: None, at: Instant::now(),
        };
        let (rows, total, groups, requested) = page(&all, Some(&capture), 0, 128, "", "physics");
        assert_eq!(total, 3);
        assert_eq!(rows.len(), 3);
        assert_eq!(groups[0]["total"], 3, "Unsupported fields remain in the visible group count");
        assert_eq!(requested.len(), 2);
        assert!(requested.contains(&supported.to_owned()));
        assert!(!requested.contains(&unsupported.to_owned()));
        let known = rows.iter().find(|row| row["id"] == supported).unwrap();
        assert_eq!(known["status"], "known");
        assert_eq!(known["value"], 0);
        let absent = rows.iter().find(|row| row["id"] == unsupported).unwrap();
        assert_eq!(absent["status"], "unavailable");
        assert_eq!(absent["value"], Value::Null, "Unsupported bytes can never become a processing-ready value");
        let (only, total, _, requested) = page(&all, Some(&capture), 0, 128, "oversized", "physics");
        assert_eq!(total, 1);
        assert_eq!(only[0]["status"], "unavailable");
        assert!(requested.is_empty(), "An unsupported-only page must cancel prior inspection subscriptions");
    }
}
