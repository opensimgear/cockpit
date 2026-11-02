use super::*;
use serde_json::{json, Value};

impl crate::gateways::GatewayManager {
    pub(crate) fn with_observation_topic<T>(&self, topic: &str,
        action: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        if let Some(id) = topic.strip_prefix("controllers.input.") {
            self.controllers.observe(&self.observations.broker(), id, action)
        } else { action() }
    }

    pub fn browser_observation_request(&self, request: &Value,
        send: impl Fn(Delivery) -> Result<(), String> + Send + 'static) -> Value {
        if let Err(error) = validate_control(request) { return json!({"ok":false,"error":error}); }
        if request["operation"] == "subscribe" {
            if let Some(topic) = request["topic"].as_str() {
                return self.with_observation_topic(topic, || Ok(self.observations.browser_request(request,send)))
                    .unwrap_or_else(|error|json!({"ok":false,"error":error}));
            }
        }
        self.observations.browser_request(request,send)
    }
}

impl Runtime {
    /// The local development bridge supplies the principal; JSON cannot select
    /// a privileged audience or request arbitrary native command execution.
    pub fn browser_request(
        &self,
        request: &Value,
        send: impl Fn(Delivery) -> Result<(), String> + Send + 'static,
    ) -> Value {
        let result = (|| -> Result<Value, String> {
            let operation = validate_control(request)?;
            let text = |key: &str| {
                request[key]
                    .as_str()
                    .filter(|value| !value.is_empty() && value.len() <= 128)
                    .ok_or_else(|| format!("Invalid observation {key}."))
            };
            match operation {
                "subscribe" => Ok(
                    json!({"ok":true,"subscriptionId":self.subscribe("browser-dev","browser",text("topic")?,send)?}),
                ),
                "acknowledge" => {
                    self.acknowledge("browser-dev", text("subscriptionId")?, text("delivery")?)?;
                    Ok(json!({"ok":true}))
                }
                _ => {
                    self.unsubscribe("browser-dev", text("subscriptionId")?)?;
                    Ok(json!({"ok":true}))
                }
            }
        })();
        result.unwrap_or_else(|error| json!({"ok":false,"error":error}))
    }
}

fn validate_control(request: &Value) -> Result<&str, String> {
    let object = request.as_object().ok_or("Invalid observation control.")?;
    if request.to_string().len() > 1024 || request["clientId"] != "browser-dev" {
        return Err("Invalid browser observation principal.".into());
    }
    let operation = request["operation"].as_str().ok_or("Missing observation operation.")?;
    let fields: &[&str] = match operation {
        "subscribe" => &["operation", "clientId", "topic"],
        "acknowledge" => &["operation", "clientId", "subscriptionId", "delivery"],
        "unsubscribe" => &["operation", "clientId", "subscriptionId"],
        _ => return Err("Unknown observation operation.".into()),
    };
    if object.len() != fields.len() || object.keys().any(|key| !fields.contains(&key.as_str())) {
        return Err("Unexpected observation control fields.".into());
    }
    for key in fields {
        if request[*key].as_str().is_none_or(|value| value.is_empty() || value.len() > 128) {
            return Err(format!("Invalid observation {key}."));
        }
    }
    Ok(operation)
}
