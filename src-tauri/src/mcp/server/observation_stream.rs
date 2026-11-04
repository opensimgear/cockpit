use super::*;
use crate::observations::{Delivery, Runtime};
use osg_streams::WireFrame;
use rmcp::service::SubscriptionContext;
use std::collections::BTreeSet;

const MAX_TOPICS: usize = 8;
const SEND_DEADLINE: Duration = Duration::from_secs(2);

pub(super) const CURSOR_KEY: &str = "opensimgear.com/observationCursor";

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cursor {
    topic: String,
    session: String,
    generation: String,
    revision: String,
}

impl Cursor {
    pub(super) fn parse(value: &Value, topic: &str) -> Result<Self, ErrorData> {
        let invalid = || ErrorData::invalid_params("Invalid Cockpit observation cursor.", None);
        let cursor: Self = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
        if cursor.topic != topic
            || cursor.session.is_empty()
            || cursor.session.len() > 128
            || cursor.session.chars().any(char::is_control)
            || !Self::counter(&cursor.generation)
            || !Self::counter(&cursor.revision)
        {
            return Err(invalid());
        }
        Ok(cursor)
    }

    fn counter(value: &str) -> bool {
        value.parse::<u64>().is_ok_and(|counter| counter.to_string() == value)
    }

    fn compare(&self, frame: &WireFrame) -> &'static str {
        if self.session != frame.session || self.generation != frame.generation {
            return "replaced";
        }
        let previous: u64 = self.revision.parse().expect("Validated cursor revision");
        let current: u64 = frame.revision.parse().expect("Native observation revision");
        match current.checked_sub(previous) {
            Some(0) => "unchanged",
            Some(1) => "advanced",
            Some(_) => "gap",
            None => "replaced",
        }
    }
}

pub(super) fn read(server: &Server, client: &str, topic: &str, cursor: Option<&Cursor>) -> Value {
    let broker = server.manager.observations.broker();
    let result = server.manager.with_observation_topic(topic, || {
        let mut reader = broker
            .subscribe(client, "mcp", topic)
            .map_err(|error| error.to_string())?;
        reader
            .try_recv()
            .map_err(|error| error.to_string())?
            .ok_or("No current observation baseline.".into())
    });
    result.map_or_else(
        |error| mcp_tools::failure("observation_unavailable", &error, false),
        |frame| match cursor {
            Some(cursor) => json!({"ok":true,"frame":frame,"cursorStatus":cursor.compare(&frame)}),
            None => json!({"ok":true,"frame":frame}),
        },
    )
}

pub(super) fn accepted(requested: &SubscriptionFilter) -> SubscriptionFilter {
    let mut seen = BTreeSet::new();
    let mut filter = SubscriptionFilter::new();
    filter.resource_subscriptions = Some(
        requested
            .resource_subscriptions
            .iter()
            .flatten()
            .filter(|uri| observation_uri::topic(uri).is_some() && seen.insert(uri.as_str()))
            .take(MAX_TOPICS)
            .cloned()
            .collect(),
    );
    filter
}

struct Readers {
    runtime: Runtime,
    client: String,
    tokens: Vec<String>,
}

impl Drop for Readers {
    fn drop(&mut self) {
        for token in &self.tokens {
            let _ = self.runtime.unsubscribe(&self.client, token);
        }
    }
}

impl Handler {
    pub(super) async fn listen_observations(
        &self,
        context: SubscriptionContext,
    ) -> Result<(), ErrorData> {
        let (server, principal) = self.principal(context.request_context(), false)?;
        let _permit = server.subscriptions.try_acquire().map_err(|_| {
            ErrorData::internal_error("Cockpit observation subscription limit reached.", None)
        })?;
        let runtime = server.manager.observations.clone();
        let client = mcp_tools::native_client(&format!("mcp-{}", principal.id));
        let mut readers = Readers {
            runtime,
            client,
            tokens: Vec::new(),
        };
        let (send, mut receive) = tokio::sync::mpsc::channel::<(String, Delivery)>(MAX_TOPICS);
        let mut seen = BTreeSet::new();
        for uri in context.accepted().resource_subscriptions.iter().flatten() {
            if !seen.insert(uri.as_str()) {
                continue;
            }
            let topic = observation_uri::topic(uri).ok_or_else(|| {
                ErrorData::invalid_params("Invalid Cockpit observation URI.", None)
            })?;
            let send = send.clone();
            let delivery_uri = uri.clone();
            let token = server
                .manager
                .with_observation_topic(topic, || {
                    readers
                        .runtime
                        .subscribe(&readers.client, "mcp", topic, move |delivery| {
                            send.try_send((delivery_uri.clone(), delivery))
                                .map_err(|_| {
                                    "MCP observation delivery queue is unavailable.".into()
                                })
                        })
                })
                .map_err(|error| ErrorData::invalid_params(error, None))?;
            readers.tokens.push(token);
        }
        drop(send);
        loop {
            let Some((uri, delivery)) = (tokio::select! {
                _ = context.cancelled() => return Ok(()),
                item = receive.recv() => item,
            }) else {
                return Ok(());
            };
            // A persisted credential may change outside this server. Recheck
            // on delivery, off the async executor and the producer thread.
            let authorization = tokio::task::spawn_blocking({
                let server = server.clone();
                let principal = principal.clone();
                move || server.authorized(&principal, false)
            });
            let authorized = tokio::select! {
                _ = context.cancelled() => return Ok(()),
                result = tokio::time::timeout(SEND_DEADLINE, authorization) => result,
            };
            if !matches!(authorized, Ok(Ok(true))) {
                return Ok(());
            }
            let mut notification =
                ResourceUpdatedNotification::new(ResourceUpdatedNotificationParam::new(uri));
            notification
                .extensions
                .insert(NotificationMetaObject(MetaObject(
                    serde_json::Map::from_iter([(
                        "opensimgear.com/observation".into(),
                        json!(delivery.frame),
                    )]),
                )));
            let notify = context
                .sink()
                .send(ServerNotification::ResourceUpdatedNotification(
                    notification,
                ));
            let sent = tokio::select! {
                _ = context.cancelled() => return Ok(()),
                sent = tokio::time::timeout(SEND_DEADLINE, notify) => sent,
            };
            if !matches!(sent, Ok(Ok(()))) {
                return Ok(());
            }
            if readers
                .runtime
                .acknowledge(
                    &readers.client,
                    &delivery.subscription_id,
                    &delivery.delivery,
                )
                .is_err()
            {
                return Ok(());
            }
        }
    }
}
