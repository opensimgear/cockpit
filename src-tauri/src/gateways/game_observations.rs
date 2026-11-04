use serde_json::Value;

impl super::GatewayManager {
    pub(super) fn inspect_game_telemetry(&self, request: &Value) -> Result<Value, String> {
        let lease = request.get("observationId").map(|value| {
            let id = value.as_str().filter(|id| id.len() == 36)
                .ok_or("Invalid telemetry observation identity.")?;
            self.observations.telemetry_lease(id)
        }).transpose()?;
        self.games.inspect_telemetry(request, lease)
    }
    pub(crate) fn with_game_detail<T>(&self,id:&str,action:impl FnOnce()->Result<T,String>)->Result<T,String> {
        self.game_read_models.with_detail(id,action)
    }
}
