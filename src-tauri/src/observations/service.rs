use super::*;
use serde_json::{json, Value};

impl Runtime {
    /// The local development bridge supplies the principal; JSON cannot select
    /// a privileged audience or request arbitrary native command execution.
    pub fn browser_request(
        &self,
        request: &Value,
        send: impl Fn(Delivery) -> Result<(), String> + Send + 'static,
    ) -> Value {
        let result = (|| -> Result<Value, String> {
            let object = request.as_object().ok_or("Invalid observation control.")?;
            if request.to_string().len() > 1024 || request["clientId"] != "browser-dev" {
                return Err("Invalid browser observation principal.".into());
            }
            let operation = request["operation"]
                .as_str()
                .ok_or("Missing observation operation.")?;
            let fields: &[&str] = match operation {
                "subscribe" => &["operation", "clientId", "topic"],
                "acknowledge" => &["operation", "clientId", "subscriptionId", "delivery"],
                "unsubscribe" => &["operation", "clientId", "subscriptionId"],
                _ => return Err("Unknown observation operation.".into()),
            };
            if object.len() != fields.len()
                || object.keys().any(|key| !fields.contains(&key.as_str()))
            {
                return Err("Unexpected observation control fields.".into());
            }
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
