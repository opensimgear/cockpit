#include "input.h"
#include <string.h>

static bool active(const osg_input *q) { return q->mode == OSG_INPUT_PREPARING || q->mode == OSG_INPUT_ARMED; }
static bool elapsed(uint64_t now, uint64_t before, uint64_t budget) { return now < before || now - before >= budget; }
void osg_input_cancel(osg_input *q, osg_input_fault fault) {
    if (q->mode != OSG_INPUT_BLOCKED) {
        q->fault = fault; q->mode = OSG_INPUT_BLOCKED;
        if (q->epoch < UINT64_MAX) q->epoch++;
    }
    q->head = 0; q->count = 0;
    if (q->runtime) osg_disarm(q->runtime);
}
static bool enqueue(void *context, const char *id, bool down, uint64_t sequence) {
    osg_input *q = context;
    if (q->mode != OSG_INPUT_ARMED) return false;
    if (q->last_edge == UINT64_MAX || sequence != q->last_edge + 1 || sequence == UINT64_MAX) {
        osg_input_cancel(q, OSG_INPUT_SEQUENCE); return false;
    }
    if (q->count == OSG_INPUT_QUEUE_CAPACITY) { osg_input_cancel(q, OSG_INPUT_OVERFLOW); return false; }
    for (size_t i = 0; i < q->runtime->panel->widget_count; i++) {
        const osg_widget *w = &q->runtime->panel->widgets[i];
        if (w->kind == OSG_BUTTON && !strcmp(id, w->reference)) {
            q->queue[(q->head + q->count) % OSG_INPUT_QUEUE_CAPACITY] = (osg_input_edge){sequence, q->now_ms, (uint8_t)i, down};
            q->count++; q->last_edge = sequence; return true;
        }
    }
    osg_input_cancel(q, OSG_INPUT_SEQUENCE); return false;
}
void osg_input_bind(osg_input *q, osg_runtime *r, bool touch_capable, uint64_t now) {
    const uint64_t epoch = q->epoch, render_start = q->render_start_ms;
    const bool rendering = q->rendering;
    const osg_panel *render_panel = q->render_panel;
    memset(q, 0, sizeof(*q));
    q->epoch = epoch < UINT64_MAX ? epoch + 1 : UINT64_MAX;
    q->rendering = rendering; q->render_start_ms = render_start; q->render_panel = render_panel;
    q->runtime = r; q->touch_capable = touch_capable; q->last_poll_ms = now; q->last_ack_ms = now; q->now_ms = now;
    r->sink = enqueue; r->sink_context = q;
    osg_disarm(r);
    if (epoch == UINT64_MAX) osg_input_cancel(q, OSG_INPUT_SEQUENCE);
}
bool osg_input_monitor(osg_input *q, uint64_t now) {
    if (!active(q)) return q->mode != OSG_INPUT_BLOCKED;
    if ((q->mode == OSG_INPUT_ARMED && !q->runtime->armed) || (q->mode == OSG_INPUT_PREPARING && !q->runtime->arming)) osg_input_cancel(q, OSG_INPUT_CANCELLED);
    else if (!q->touch_valid || !q->touch_capable) osg_input_cancel(q, OSG_INPUT_TOUCH_FAILED);
    else if (elapsed(now, q->last_poll_ms, OSG_TOUCH_LEASE_MS)) osg_input_cancel(q, OSG_INPUT_UI_EXPIRED);
    else if (elapsed(now, q->last_ack_ms, OSG_ACK_LEASE_MS)) osg_input_cancel(q, OSG_INPUT_HOST_EXPIRED);
    else if (q->rendering && elapsed(now, q->render_start_ms, OSG_TOUCH_LEASE_MS)) osg_input_cancel(q, OSG_INPUT_DISPLAY_FAILED);
    else if (q->count && elapsed(now, q->queue[q->head].created_ms, OSG_INPUT_MAX_AGE_MS)) osg_input_cancel(q, OSG_INPUT_BACKLOG);
    return q->mode != OSG_INPUT_BLOCKED;
}
void osg_input_poll(osg_input *q, bool valid, bool down, int x, int y, uint64_t now) {
    (void)osg_input_monitor(q, now); /* A late poll cannot hide a missed lease. */
    q->now_ms = now; q->last_poll_ms = now; q->touch_valid = valid;
    if (!valid) { if (active(q)) osg_input_cancel(q, OSG_INPUT_TOUCH_FAILED); else osg_disarm(q->runtime); return; }
    if (q->mode == OSG_INPUT_PREPARING && down) osg_input_cancel(q, OSG_INPUT_CANCELLED);
    const bool armed = q->runtime->armed;
    osg_touch(q->runtime, down, x, y);
    if (armed && !q->runtime->armed) osg_input_cancel(q, OSG_INPUT_CANCELLED);
}
void osg_input_render(osg_input *q, const osg_panel *panel, bool finished, bool success, uint64_t now) {
    if (!finished) { q->rendering = true; q->render_panel = panel; q->render_start_ms = now; return; }
    q->rendering = false;
    if (panel == q->runtime->panel) q->display_ready = success;
    if (!success && active(q)) osg_input_cancel(q, OSG_INPUT_DISPLAY_FAILED);
}
bool osg_input_can_arm(const osg_input *q, uint64_t now) {
    if (q->mode != OSG_INPUT_DISARMED || !q->touch_capable || !q->touch_valid || !q->display_ready ||
        !q->runtime->connected || q->runtime->pointer_down || q->runtime->await_release || q->epoch >= UINT64_MAX - 1 ||
        elapsed(now, q->last_poll_ms, OSG_TOUCH_LEASE_MS) || elapsed(now, q->last_ack_ms, OSG_ACK_LEASE_MS) ||
        (q->rendering && elapsed(now, q->render_start_ms, OSG_TOUCH_LEASE_MS))) return false;
    return true;
}
bool osg_input_prepare(osg_input *q, uint64_t now, uint64_t *epoch) {
    if (!osg_input_can_arm(q, now)) return false;
    q->epoch++; q->mode = OSG_INPUT_PREPARING; q->head = q->count = 0; q->last_edge = 0;
    q->runtime->sequence = 0; q->runtime->arming = true; *epoch = q->epoch;
    return true;
}
bool osg_input_commit(osg_input *q, uint64_t epoch, uint64_t request_ms, uint64_t now) {
    if (q->epoch != epoch || q->mode != OSG_INPUT_PREPARING || request_ms > now) return false;
    q->last_ack_ms = request_ms;
    if (!osg_input_monitor(q, now)) return false;
    q->runtime->arming = false;
    if (!osg_live_arm(q->runtime)) { osg_input_cancel(q, OSG_INPUT_CANCELLED); return false; }
    q->mode = OSG_INPUT_ARMED; return true;
}
bool osg_input_peek(osg_input *q, osg_input_edge *out, size_t capacity, size_t *count, uint64_t *epoch, uint64_t now) {
    *count = 0;
    if (!osg_input_monitor(q, now) || q->mode != OSG_INPUT_ARMED) return false;
    *count = q->count < capacity ? q->count : capacity; *epoch = q->epoch;
    for (size_t i = 0; i < *count; i++) out[i] = q->queue[(q->head + i) % OSG_INPUT_QUEUE_CAPACITY];
    return true;
}
bool osg_input_accept(osg_input *q, uint64_t epoch, size_t count) {
    if (q->mode != OSG_INPUT_ARMED || q->epoch != epoch || !count || count > q->count) return false;
    q->head = (q->head + count) % OSG_INPUT_QUEUE_CAPACITY; q->count -= count;
    return true;
}
