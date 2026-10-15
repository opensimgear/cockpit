fn on_start(ctx) { transaction(#{armed: false}, [button("result", false)], []) }
fn on_event(ctx) {
    let first = ctx.inputs.left.trigger.value;
    let second = ctx.inputs.right.trigger.value;
    let armed = ctx.state.armed || (!first && !second);
    transaction(#{armed: armed}, [button("result", armed && first && second)], [])
}
fn on_timer(ctx) { transaction(ctx.state, [], []) }
