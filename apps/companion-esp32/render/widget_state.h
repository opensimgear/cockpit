#ifndef OSG_WIDGET_STATE_H
#define OSG_WIDGET_STATE_H
#include "widget_source.h"

/* Borrowed, already identity/type/unit/range-validated feedback for one draw
 * snapshot. The publication/session layer maps logical IDs to definition ports.
 * A null value is an absent sample. Preserve simulator origin independently of
 * provenance; a host/script sample cannot confirm an indicator. */
typedef struct { const osg_sample *value; bool simulator; } osg_widget_sample_view;
typedef struct {
    osg_widget_sample_view outputs[OSG_WIDGET_SOURCE_PORTS];
    uint64_t now_ms;
    bool connected, enabled, pressed;
} osg_widget_state_context;

osg_widget_state osg_widget_state_resolve(const osg_widget_component *, const osg_widget_state_context *);
/* The visual gate is a necessary input condition, not an arming decision.
 * Existing session ownership, neutral baselines and leases remain authoritative. */
bool osg_widget_gate_open(const osg_widget_component *, const osg_widget_state_context *);
bool osg_widget_indicator_active(const osg_widget_layer *, const osg_widget_state_context *);
double osg_widget_gauge_fraction(const osg_widget_layer *, const osg_widget_state_context *);
#endif
