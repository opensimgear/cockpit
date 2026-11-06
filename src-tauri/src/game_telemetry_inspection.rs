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
    pub values: crate::native_capture::CapturedValues,
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
                crate::game_channels::captured_values(game, &snapshot.values, Instant::now())
            } else {
                BTreeMap::new()
            },
            ages: BTreeMap::new(),
            stale_ids: BTreeSet::new(),
            message: None,
            at: Instant::now(),
        }
    }
    /// Host projection age changes as time passes. Original packet evidence,
    /// selected values and their contributor leases determine whether a new
    /// telemetry revision is needed.
    pub(crate) fn same_observation(&self, other: &Self) -> bool {
        self.status == other.status
            && self.source == other.source
            && self.context == other.context
            && self.packet == other.packet
            && self.age_ms.is_some() == other.age_ms.is_some()
            && self.values == other.values
            && self.stale_ids == other.stale_ids
            && self.message == other.message
            && self.ages.len() == other.ages.len()
            && self.ages.iter().all(|(id, age)| {
                other.ages.get(id).is_some_and(|previous| previous.is_some() == age.is_some())
            })
    }

    pub(crate) fn next_deadline(&self, process: Option<Instant>, now: Instant) -> Option<Instant> {
        use crate::native_capture::Validity;
        // Both existing age fences include equality. Native contributor leases
        // are exclusive and must withdraw at their exact original deadline.
        let inclusive = Duration::from_secs(2) + Duration::from_nanos(1);
        if process.is_some_and(|process| now >= process) { return None; }
        let cache = self.at.checked_add(inclusive)?;
        if now >= cache { return process; }
        let mut deadline = process.map_or(cache, |process| process.min(cache));
        if self.status == "live" {
            for value in self.values.values().filter(|value| value.is_current(now)) {
                for contribution in value.contributors().into_iter().flatten() {
                    let expires = match &contribution.validity {
                        Validity::Sampled { valid_until } => *valid_until,
                        Validity::Latched(lease) => lease.valid_until,
                    };
                    deadline = deadline.min(expires);
                }
            }
        }
        Some(deadline)
    }
    pub(crate) fn expired(&self) -> bool {
        self.expired_at(Instant::now())
    }

    pub(crate) fn expired_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.at) > Duration::from_secs(2)
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
/// Prepared channel metadata and demand for one exact inspector page.
/// Source updates can project this selection without filtering the catalogue again.
pub(crate) struct Page {
    channels: Vec<Value>,
    pub total: usize,
    pub groups: Vec<Value>,
    pub requested: Vec<String>,
}

impl Page {
    pub(crate) fn prepare(
        all: &[Value],
        offset: usize,
        limit: usize,
        search: &str,
        group: &str,
    ) -> Self {
        let mut groups = BTreeMap::<String, usize>::new();
        let mut channels = Vec::new();
        let mut total = 0;
        for channel in all {
            let category = category(channel);
            *groups.entry(category.clone()).or_default() += 1;
            if !group.is_empty() && category != group {
                continue;
            }
            if !search.is_empty()
                && !["id", "label", "nativeField", "description"].iter().any(|key| {
                    channel[key].as_str().is_some_and(|value| value.to_lowercase().contains(search))
                })
            {
                continue;
            }
            if total >= offset && channels.len() < limit {
                channels.push(channel.clone());
            }
            total += 1;
        }
        let groups = groups.into_iter()
            .map(|(id, total)| json!({"label":group_label(&id),"id":id,"total":total}))
            .collect();
        let requested = channels.iter()
            .filter(|channel| channel["supported"] != false)
            .map(|channel| channel["id"].as_str().unwrap_or("").to_owned())
            .collect();
        Self { channels, total, groups, requested }
    }

    pub(crate) fn project(&self, capture: Option<&Capture>, now: Instant) -> Vec<Value> {
        let capture_expired = capture.is_some_and(|capture| capture.expired_at(now));
        let live = capture.filter(|capture| capture.status == "live" && !capture_expired);
        let age = capture.map(|capture| now.saturating_duration_since(capture.at).as_millis() as u64);
        self.channels.iter().map(|channel| {
            let id = channel["id"].as_str().unwrap_or("");
            let projected = live.and_then(|capture| capture.values.get(id));
            let current = projected.filter(|value| value.is_current(now));
            let value = current.map(|value| value.value())
                .filter(|value| value.is_number() || value.is_string() || value.is_boolean());
            let status = if channel["supported"] == false {
                "unavailable"
            } else if capture.is_some_and(|capture| capture.stale_ids.contains(id))
                || projected.is_some() && current.is_none()
            {
                "stale"
            } else if value.is_some() {
                "known"
            } else if capture.is_some_and(|capture| capture_expired || matches!(capture.status.as_str(), "stale" | "paused")) {
                "stale"
            } else {
                "unknown"
            };
            let age = if let Some(value) = projected.filter(|value| value.contributors().is_some()) {
                value.authority_age_ms(now)
            } else {
                capture.and_then(|capture| capture.ages.get(id).copied().unwrap_or(capture.age_ms))
                    .zip(age).map(|(source, elapsed)| source.saturating_add(elapsed))
            };
            json!({
                "id": id,
                "label": bounded(&channel["label"], id, 256),
                "category": category(channel),
                "group": channel["group"].as_str().unwrap_or("telemetry"),
                "type": channel["type"].as_str().unwrap_or("number"),
                "unit": channel["unit"].as_str(),
                "status": status,
                "value": if status == "known" { value.cloned().unwrap_or(Value::Null) } else { Value::Null },
                "ageMs": age,
                "nativeId": channel["nativeId"].as_str().unwrap_or(id),
                "nativeField": channel["nativeField"].as_str().or_else(|| channel["dataref"].as_str()),
                "description": channel["note"].as_str().or_else(|| channel["description"].as_str()),
                "provider": capture.map(|capture| capture.source.as_str()).unwrap_or(""),
            })
        }).collect()
    }
}

#[cfg(test)]
pub(crate) fn page(
    all: &[Value],
    capture: Option<&Capture>,
    offset: usize,
    limit: usize,
    search: &str,
    group: &str,
) -> (Vec<Value>, usize, Vec<Value>, Vec<String>) {
    let page = Page::prepare(all, offset, limit, search, group);
    (page.project(capture, Instant::now()), page.total, page.groups, page.requested)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_native_values_expire_from_original_proof_before_the_cache_timeout() {
        use crate::native_capture::{CapturedValue,Contribution,InputId,Stamp,Validity};
        let old=Instant::now()-Duration::from_secs(1);
        let expires=old+Duration::from_millis(100);
        let value=CapturedValue::native(json!(0),vec![std::sync::Arc::new(Contribution {
            input:InputId::Channel("native-rpm".into()),capture:Stamp {
                producer_id:"reader".into(),generation:1,sequence:5,byte_offset:None,captured_at:old,
            },validity:Validity::Sampled {valid_until:expires},
        })],old).unwrap();
        let capture=Capture {status:"live".into(),source:"fixture-native".into(),context:json!({}),
            packet:json!(5),age_ms:Some(0),values:BTreeMap::from([("engine.rpm".into(),value)]),
            ages:BTreeMap::new(),stale_ids:BTreeSet::new(),message:None,at:old};
        let catalogue=vec![json!({"id":"engine.rpm","type":"number"})];
        let prepared=Page::prepare(&catalogue,0,1,"","");
        let mut projection = capture.clone();
        projection.at += Duration::from_millis(50);
        projection.age_ms = Some(50);
        assert!(capture.same_observation(&projection));
        projection.packet = json!(6);
        assert!(!capture.same_observation(&projection));
        projection.packet = capture.packet.clone();
        projection.status = "paused".into();
        assert!(!capture.same_observation(&projection));
        let process = old + Duration::from_secs(2) + Duration::from_nanos(1);
        assert_eq!(capture.next_deadline(Some(process), old), Some(expires));
        assert_eq!(capture.next_deadline(Some(process), expires), Some(process));
        assert_eq!(capture.next_deadline(None, expires), Some(process));
        let live=prepared.project(Some(&capture),expires-Duration::from_nanos(1));
        assert_eq!(live[0]["status"],"known");
        assert_eq!(live[0]["value"],0);
        let stale=prepared.project(Some(&capture),expires);
        assert_eq!(stale[0]["status"],"stale");
        assert_eq!(stale[0]["value"],Value::Null);
        assert_eq!(stale[0]["ageMs"],100);
        assert!(!capture.expired_at(expires),"the expiry must come from native proof");
    }
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
            values: BTreeMap::from([("engine.rpm".into(), crate::native_capture::CapturedValue::fixture(json!(0)))]),
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
            values: BTreeMap::from([(supported.into(), crate::native_capture::CapturedValue::fixture(json!(0))), (unsupported.into(), crate::native_capture::CapturedValue::fixture(json!(999)))]),
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
