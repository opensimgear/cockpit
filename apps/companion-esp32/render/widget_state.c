#include "widget_state.h"
#include <math.h>

static const osg_sample *sample(const osg_widget_state_context *context, int index) {
    return context && index >= 0 && index < OSG_WIDGET_SOURCE_PORTS ? context->outputs[index].value : NULL;
}
static const osg_sample *fresh(const osg_widget_state_context *context, int index) {
    const osg_sample *value = sample(context, index);
    if (!value || !value->available || value->source_status != OSG_FEEDBACK_AUTOMATIC || value->received_ms > context->now_ms || value->source_age_ms >= OSG_STALE_MS || context->now_ms - value->received_ms >= OSG_STALE_MS - value->source_age_ms) return NULL;
    return value;
}
bool osg_widget_gate_open(const osg_widget_component *component, const osg_widget_state_context *context) {
    if (!component || !context) return false;
    if (component->enabled_output < 0) return true;
    const osg_sample *gate = fresh(context, component->enabled_output);
    return gate && gate->kind == OSG_BOOL && gate->boolean;
}
osg_widget_state osg_widget_state_resolve(const osg_widget_component *component, const osg_widget_state_context *context) {
    if (!component || !context || !context->connected) return OSG_WIDGET_DISCONNECTED;
    if (!context->enabled) return OSG_WIDGET_DISABLED;
    const osg_sample *gate = fresh(context, component->enabled_output);
    if (gate && gate->kind == OSG_BOOL && !gate->boolean) return OSG_WIDGET_DISABLED;
    const int ports[] = {component->confirmed_output, component->warning_output, component->enabled_output};
    for (unsigned i = 0; i < 3; i++) {
        const osg_sample *value = sample(context, ports[i]);
        if (value && value->source_status == OSG_FEEDBACK_DISCONNECTED) return OSG_WIDGET_DISCONNECTED;
    }
    const osg_sample *warning = fresh(context, component->warning_output);
    if (warning && warning->kind == OSG_BOOL && warning->boolean) return OSG_WIDGET_WARNING;
    if (context->pressed) return OSG_WIDGET_PRESSED;
    for (unsigned i = 0; i < 3; i++) if (ports[i] >= 0) {
        const osg_sample *value = sample(context, ports[i]);
        if (!value || value->source_status == OSG_FEEDBACK_UNKNOWN || (value->source_status == OSG_FEEDBACK_AUTOMATIC && (!value->available || value->kind != OSG_BOOL))) return OSG_WIDGET_UNKNOWN;
    }
    for (unsigned i = 0; i < 3; i++) if (ports[i] >= 0 && !fresh(context, ports[i])) return OSG_WIDGET_STALE;
    const osg_sample *confirmed = fresh(context, component->confirmed_output);
    return confirmed && confirmed->kind == OSG_BOOL && confirmed->boolean && confirmed->provenance == OSG_GAME_CONFIRMED && context->outputs[component->confirmed_output].simulator ? OSG_WIDGET_ACTIVE : OSG_WIDGET_NORMAL;
}
bool osg_widget_indicator_active(const osg_widget_layer *layer, const osg_widget_state_context *context) {
    if (!layer || layer->kind != OSG_LAYER_INDICATOR) return false;
    const osg_sample *value = fresh(context, layer->output);
    return value && value->kind == OSG_BOOL && value->boolean && value->provenance == OSG_GAME_CONFIRMED && context->outputs[layer->output].simulator;
}
double osg_widget_gauge_fraction(const osg_widget_layer *layer, const osg_widget_state_context *context) {
    if (!layer || layer->kind != OSG_LAYER_GAUGE || !isfinite(layer->min) || !isfinite(layer->max) || layer->max <= layer->min) return 0;
    const osg_sample *value = fresh(context, layer->output);
    if (!value || value->kind != OSG_NUMBER || !isfinite(value->number)) return 0;
    return fmax(0, fmin(1, (value->number - layer->min) / (layer->max - layer->min)));
}
