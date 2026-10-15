fn on_start(ctx) { transaction(#{active: false}, [button("result", false)], []) }
fn on_event(ctx) {
    let value = ctx.inputs.left.x.value;
    let active = if ctx.state.active { value > 12000 } else { value >= 16000 };
    transaction(#{active: active}, [button("result", active)], [])
}
fn on_timer(ctx) { transaction(ctx.state, [], []) }
