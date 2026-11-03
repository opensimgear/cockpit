impl super::GatewayManager {
    pub(crate) fn with_game_detail<T>(&self,id:&str,action:impl FnOnce()->Result<T,String>)->Result<T,String> {
        self.game_read_models.with_detail(id,action)
    }
}
