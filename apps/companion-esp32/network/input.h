#ifndef OSG_INPUT_H
#define OSG_INPUT_H
#include "companion.h"
#define OSG_INPUT_QUEUE_CAPACITY 64
#define OSG_INPUT_BATCH_EDGES 16
#define OSG_INPUT_MAX_AGE_MS 250
#define OSG_TOUCH_LEASE_MS 600
#define OSG_ACK_LEASE_MS 1000
#define OSG_INPUT_BATCH_INTERVAL_MS 10

typedef enum { OSG_INPUT_DISARMED, OSG_INPUT_PREPARING, OSG_INPUT_ARMED, OSG_INPUT_BLOCKED } osg_input_mode;
typedef enum { OSG_INPUT_OK, OSG_INPUT_CANCELLED, OSG_INPUT_TOUCH_FAILED, OSG_INPUT_UI_EXPIRED, OSG_INPUT_HOST_EXPIRED, OSG_INPUT_DISPLAY_FAILED, OSG_INPUT_OVERFLOW, OSG_INPUT_BACKLOG, OSG_INPUT_SEQUENCE } osg_input_fault;
typedef struct { uint64_t sequence, created_ms; uint8_t widget; bool down; osg_value_kind kind; double number; } osg_input_edge;
typedef struct {
    osg_runtime *runtime;
    osg_input_edge queue[OSG_INPUT_QUEUE_CAPACITY];
    size_t head, count;
    uint64_t epoch, last_edge, last_poll_ms, last_ack_ms, render_start_ms, now_ms;
    const osg_panel *render_panel;
    osg_input_mode mode;
    osg_input_fault fault;
    bool touch_capable, touch_valid, display_ready, rendering;
} osg_input;
/* All calls (including the runtime's sink callback) run under the application's
 * short state mutex. No function performs I/O, allocation or rendering. */
void osg_input_bind(osg_input *, osg_runtime *, bool touch_capable, uint64_t now);
void osg_input_cancel(osg_input *, osg_input_fault);
void osg_input_poll(osg_input *, bool valid, bool down, int x, int y, uint64_t now);
void osg_input_render(osg_input *, const osg_panel *, bool finished, bool success, uint64_t now);
/* A completed display write renews progress, never arming or completing a frame. */
void osg_input_render_progress(osg_input *, const osg_panel *, uint64_t now);
bool osg_input_monitor(osg_input *, uint64_t now);
bool osg_input_can_arm(const osg_input *, uint64_t now);
bool osg_input_prepare(osg_input *, uint64_t now, uint64_t *epoch);
bool osg_input_commit(osg_input *, uint64_t epoch, uint64_t request_ms, uint64_t now);
bool osg_input_peek(osg_input *, osg_input_edge *, size_t capacity, size_t *count, uint64_t *epoch, uint64_t now);
bool osg_input_accept(osg_input *, uint64_t epoch, size_t count);
#endif
