#ifndef OSG_DIAGNOSTICS_H
#define OSG_DIAGNOSTICS_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

/* Fixed storage, no allocation, formatting or I/O in a measured path. ESP-IDF
 * builds protect updates with a short critical section; native tests are single
 * threaded. Histograms report conservative powers-of-two upper bounds. */
#define OSG_DIAG_BUCKETS 32
typedef enum {
    OSG_DIAG_LCD_SPI, OSG_DIAG_TOUCH_SPI, OSG_DIAG_LCD_WRITE,
    OSG_DIAG_LCD_WAIT, OSG_DIAG_TOUCH_WAIT, OSG_DIAG_TOUCH_POLL,
    OSG_DIAG_TOUCH_INTERVAL, OSG_DIAG_RENDER, OSG_DIAG_PRESENT,
    OSG_DIAG_REQUEST, OSG_DIAG_SOURCE_ACK, OSG_DIAG_INPUT_ACK,
    OSG_DIAG_METRIC_COUNT
} osg_diag_metric;
typedef enum { OSG_DIAG_SERIAL_TX, OSG_DIAG_SERIAL_RX, OSG_DIAG_WS_TX, OSG_DIAG_WS_RX, OSG_DIAG_BYTE_COUNT } osg_diag_bytes;
typedef enum { OSG_DIAG_MAIN_STACK, OSG_DIAG_TOUCH_STACK, OSG_DIAG_NETWORK_STACK, OSG_DIAG_TASK_COUNT } osg_diag_task;
typedef struct {
    uint32_t count, failed, dropped, histogram[OSG_DIAG_BUCKETS];
    uint64_t total_us, max_us, bytes;
} osg_diag_stat;
typedef struct {
    osg_diag_stat metrics[OSG_DIAG_METRIC_COUNT];
    uint64_t bytes[OSG_DIAG_BYTE_COUNT];
    uint32_t memory_samples, free_internal_bytes, minimum_internal_bytes,
        largest_internal_bytes, smallest_largest_internal_bytes;
    uint32_t stack_samples[OSG_DIAG_TASK_COUNT], stack_min_bytes[OSG_DIAG_TASK_COUNT];
    uint32_t queue_max;
    uint64_t pending_input_max_us, touch_age_max_us, ack_age_max_us, render_age_max_us;
} osg_diagnostics;

void osg_diagnostics_reset(void);
void osg_diagnostics_record(osg_diag_metric, uint64_t elapsed_us, size_t bytes, bool success);
void osg_diagnostics_bytes(osg_diag_bytes, size_t bytes);
void osg_diagnostics_memory(uint32_t free_internal, uint32_t minimum_internal, uint32_t largest_internal);
void osg_diagnostics_stack(osg_diag_task, uint32_t free_bytes);
/* Ages are sampled while input is active, before monitor/poll can clear a late
 * queue or refresh a timestamp. They observe leases and never alter them. */
void osg_diagnostics_health(size_t queued, uint64_t pending_us, uint64_t touch_age_us, uint64_t ack_age_us, uint64_t render_age_us);
void osg_diagnostics_snapshot(osg_diagnostics *out);
uint64_t osg_diagnostics_percentile(const osg_diag_stat *, unsigned percentile);
const char *osg_diagnostics_metric_name(osg_diag_metric);
#endif
