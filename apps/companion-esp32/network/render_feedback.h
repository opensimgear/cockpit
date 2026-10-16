#ifndef OSG_RENDER_FEEDBACK_H
#define OSG_RENDER_FEEDBACK_H
#include "wire.h"
/* Render-v2 only, with the immutable logical shim produced by engine.c.
 * Output indices enumerate non-button shim widgets in source output order.
 * Reconstructs existing full feedback and calls osg_ack; it never assigns samples.
 * max_ack_bytes is the negotiated full-ACK budget (e.g. Sunton startup3072,
 * steady512, Waveshare4096). All failures leave runtime/session unchanged. */
bool osg_render_feedback_ack(const cJSON *ack,osg_live_session *session,const char *sequence,
                             osg_runtime *runtime,uint64_t now,size_t max_ack_bytes);
#endif
