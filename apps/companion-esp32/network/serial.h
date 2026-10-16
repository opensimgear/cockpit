#ifndef OSG_SERIAL_H
#define OSG_SERIAL_H
#include <stdbool.h>
#include <stdint.h>
#define OSG_SERIAL_HEALTH_RESERVE_BYTES 512u
typedef struct {
    uint32_t fifo_overflows,buffer_full,frame_errors,parity_errors,breaks,queue_full_observations;
    uint32_t request_timeouts,invalid_json,reply_mismatch,deferred_full,invalid_reply,write_failures,frame_overflows;
} osg_serial_health;
typedef enum { OSG_SERIAL_TIMEOUT,OSG_SERIAL_JSON,OSG_SERIAL_IDENTITY,OSG_SERIAL_DEFERRED,
    OSG_SERIAL_REPLY,OSG_SERIAL_WRITE,OSG_SERIAL_FRAME,OSG_SERIAL_FAILURE_COUNT } osg_serial_failure;
bool osg_serial_start(void);
/* Network-task only. Drain at most16 driver events without waiting. Counts are
 * saturating boot-lifetime observations; queue saturation means events may be
 * missing, so zero error counts alone do not prove an error-free UART. */
void osg_serial_health_poll(void);
void osg_serial_health_snapshot(osg_serial_health *);
void osg_serial_health_failure(osg_serial_failure);
#endif
