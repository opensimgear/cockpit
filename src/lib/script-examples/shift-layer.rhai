fn on_start(ctx) { transaction(#{armed: false}, [button("result", false)], []) }
fn on_event(ctx) {
    let shift = ctx.inputs.left.trigger.value;
    let pressed = ctx.inputs.right.trigger.value;
    let armed = ctx.state.armed || !pressed;
    transaction(#{armed: armed}, [button("result", armed && shift && pressed)], [])
}
fn on_timer(ctx) { transaction(ctx.state, [], []) }
