#include "diagnostics.h"
#include <string.h>
#ifdef ESP_PLATFORM
#include "freertos/FreeRTOS.h"
static portMUX_TYPE guard = portMUX_INITIALIZER_UNLOCKED;
#define LOCK() portENTER_CRITICAL(&guard)
#define UNLOCK() portEXIT_CRITICAL(&guard)
#else
#define LOCK() ((void)0)
#define UNLOCK() ((void)0)
#endif

static osg_diagnostics counters;
_Static_assert(sizeof(osg_diagnostics) <= 2304, "Baseline counters must stay within their fixed internal-RAM allowance");
static uint64_t add(uint64_t a, uint64_t b) { return UINT64_MAX - a < b ? UINT64_MAX : a + b; }
static uint64_t maximum(uint64_t a, uint64_t b) { return a > b ? a : b; }
void osg_diagnostics_reset(void) { LOCK(); memset(&counters, 0, sizeof(counters)); UNLOCK(); }
void osg_diagnostics_record(osg_diag_metric metric, uint64_t us, size_t bytes, bool success) {
    if ((unsigned)metric >= OSG_DIAG_METRIC_COUNT) return;
    unsigned bucket = 0;
    for (uint64_t upper = 1; us > upper && bucket < OSG_DIAG_BUCKETS - 1; upper <<= 1) bucket++;
    LOCK();
    osg_diag_stat *s = &counters.metrics[metric];
    if (s->count == UINT32_MAX) { if (s->dropped < UINT32_MAX) s->dropped++; }
    else {
        s->count++; if (!success) s->failed++; s->histogram[bucket]++;
        s->total_us = add(s->total_us, us); s->max_us = maximum(s->max_us, us); s->bytes = add(s->bytes, bytes);
    }
    UNLOCK();
}
void osg_diagnostics_bytes(osg_diag_bytes channel, size_t bytes) {
    if ((unsigned)channel >= OSG_DIAG_BYTE_COUNT) return;
    LOCK(); counters.bytes[channel] = add(counters.bytes[channel], bytes); UNLOCK();
}
void osg_diagnostics_memory(uint32_t free_bytes, uint32_t minimum_bytes, uint32_t largest_bytes) {
    LOCK();
    if (!counters.memory_samples || largest_bytes < counters.smallest_largest_internal_bytes)
        counters.smallest_largest_internal_bytes = largest_bytes;
    if (counters.memory_samples < UINT32_MAX) counters.memory_samples++;
    counters.free_internal_bytes = free_bytes; counters.minimum_internal_bytes = minimum_bytes;
    counters.largest_internal_bytes = largest_bytes;
    UNLOCK();
}
void osg_diagnostics_stack(osg_diag_task task, uint32_t bytes) {
    if ((unsigned)task >= OSG_DIAG_TASK_COUNT) return;
    LOCK();
    if (!counters.stack_samples[task] || bytes < counters.stack_min_bytes[task]) counters.stack_min_bytes[task] = bytes;
    if (counters.stack_samples[task] < UINT32_MAX) counters.stack_samples[task]++;
    UNLOCK();
}
void osg_diagnostics_health(size_t queued, uint64_t pending, uint64_t touch, uint64_t ack, uint64_t render) {
    LOCK();
    counters.queue_max = (uint32_t)maximum(counters.queue_max, queued > UINT32_MAX ? UINT32_MAX : queued);
    counters.pending_input_max_us = maximum(counters.pending_input_max_us, pending);
    counters.touch_age_max_us = maximum(counters.touch_age_max_us, touch);
    counters.ack_age_max_us = maximum(counters.ack_age_max_us, ack);
    counters.render_age_max_us = maximum(counters.render_age_max_us, render);
    UNLOCK();
}
void osg_diagnostics_snapshot(osg_diagnostics *out) { if (out) { LOCK(); *out = counters; UNLOCK(); } }
uint64_t osg_diagnostics_percentile(const osg_diag_stat *s, unsigned percentile) {
    if (!s || !s->count || !percentile || percentile > 100) return 0;
    const uint64_t rank = ((uint64_t)s->count * percentile + 99) / 100;
    uint64_t count = 0;
    for (unsigned i = 0; i < OSG_DIAG_BUCKETS; i++) {
        count += s->histogram[i];
        if (count >= rank) return i == OSG_DIAG_BUCKETS - 1 ? UINT64_MAX : UINT64_C(1) << i;
    }
    return UINT64_MAX;
}
const char *osg_diagnostics_metric_name(osg_diag_metric metric) {
    static const char *const names[] = {"lcd-spi", "touch-spi", "lcd-write", "lcd-bus-wait", "touch-bus-wait",
        "touch-poll", "touch-poll-interval", "render", "present", "host-request", "source-ack", "input-source-ack"};
    return (unsigned)metric < OSG_DIAG_METRIC_COUNT ? names[metric] : NULL;
}
