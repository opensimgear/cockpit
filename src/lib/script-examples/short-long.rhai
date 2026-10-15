fn on_start(ctx) { transaction(#{armed: false, held: false, since: 0, heldLong: false}, [button("result", false)], []) }
fn on_event(ctx) {
    let pressed = ctx.inputs.left.trigger.value;
    let state = ctx.state;
    if !pressed {
        let short = state.held && !state.heldLong;
        state.armed = true; state.held = false; state.heldLong = false;
        return transaction(state, [button("result", short)], if short { [timer("release", 5000)] } else { [] });
    }
    if state.armed && !state.held {
        state.held = true; state.since = ctx.nowUs;
        return transaction(state, [button("result", false)], [timer("hold", 300000)]);
    }
    transaction(state, [button("result", state.heldLong)], if state.held && !state.heldLong { [timer("hold", max(1000,300000-(ctx.nowUs-state.since)))] } else { [] })
}
fn on_timer(ctx) {
    let state = ctx.state;
    if ctx.event.timer == "hold" && state.held { state.heldLong = true; }
    transaction(state, [button("result", state.heldLong)], [])
}
