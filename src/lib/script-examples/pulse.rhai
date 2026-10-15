fn on_start(ctx) { transaction(#{armed: false, held: false, active: false, endUs: 0}, [button("result", false)], []) }
fn on_event(ctx) {
    let pressed = ctx.inputs.left.trigger.value;
    let state = ctx.state;
    if !pressed { state.armed = true; }
    if pressed && !state.held && state.armed { state.active = true; state.endUs = ctx.nowUs+5000; }
    state.held = pressed;
    if state.active && ctx.nowUs >= state.endUs { state.active = false; }
    transaction(state, [button("result", state.active)], if state.active { [timer("release", max(1000,state.endUs-ctx.nowUs))] } else { [] })
}
fn on_timer(ctx) { let state=ctx.state; state.active=false; transaction(state,[button("result",false)],[]) }
