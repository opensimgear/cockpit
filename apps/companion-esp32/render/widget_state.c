#include "widget_state.h"
#include <math.h>
#include <string.h>

void osg_widget_runtime_context(const osg_runtime *runtime, size_t index, uint64_t now, osg_widget_state_context *out) {
    memset(out, 0, sizeof(*out)); out->now_ms = now;
    if (!runtime || !runtime->panel || index >= runtime->panel->widget_count) return;
    const osg_widget *widget = &runtime->panel->widgets[index];
    if (widget->kind != OSG_COMPONENT || !widget->component || !widget->component->definition) return;
    const osg_interaction_kind interaction = widget->component->definition->interaction;
    out->connected = runtime->connected;
    out->enabled = interaction == OSG_INTERACTION_NONE || interaction == OSG_INTERACTION_NAVIGATE || runtime->armed;
    out->pressed = runtime->active == (int)index && interaction != OSG_INTERACTION_NONE && interaction != OSG_INTERACTION_NAVIGATE;
    for (unsigned p = 0; p < widget->component->definition->output_count; p++) {
        const unsigned slot = widget->component->outputs[p].slot;
        if (slot >= OSG_MAX_WIDGETS) continue;
        const osg_sample *sample = &runtime->samples[slot];
        if (sample->received) out->outputs[p] = (osg_widget_sample_view){sample, sample->simulator};
    }
}

static const osg_sample *sample(const osg_widget_state_context *context, int index) {
    return context && index >= 0 && index < OSG_WIDGET_SOURCE_PORTS ? context->outputs[index].value : NULL;
}
const osg_sample *osg_widget_current_sample(const osg_widget_state_context *context, int index) {
    const osg_sample *value = sample(context, index);
    if (!value || !value->available || value->source_status != OSG_FEEDBACK_AUTOMATIC || value->received_ms > context->now_ms || value->source_age_ms >= OSG_STALE_MS || context->now_ms - value->received_ms >= OSG_STALE_MS - value->source_age_ms) return NULL;
    return value;
}
bool osg_widget_gate_open(const osg_widget_component *component, const osg_widget_state_context *context) {
    if (!component || !context) return false;
    if (component->enabled_output < 0) return true;
    const osg_sample *gate = osg_widget_current_sample(context, component->enabled_output);
    return gate && gate->kind == OSG_BOOL && gate->boolean;
}
osg_widget_state osg_widget_state_resolve(const osg_widget_component *component, const osg_widget_state_context *context) {
    if (!component || !context || !context->connected) return OSG_WIDGET_DISCONNECTED;
    if (!context->enabled) return OSG_WIDGET_DISABLED;
    const osg_sample *gate = osg_widget_current_sample(context, component->enabled_output);
    if (gate && gate->kind == OSG_BOOL && !gate->boolean) return OSG_WIDGET_DISABLED;
    const int ports[] = {component->confirmed_output, component->warning_output, component->enabled_output};
    for (unsigned i = 0; i < 3; i++) {
        const osg_sample *value = sample(context, ports[i]);
        if (value && value->source_status == OSG_FEEDBACK_DISCONNECTED) return OSG_WIDGET_DISCONNECTED;
    }
    const osg_sample *warning = osg_widget_current_sample(context, component->warning_output);
    if (warning && warning->kind == OSG_BOOL && warning->boolean) return OSG_WIDGET_WARNING;
    if (context->pressed) return OSG_WIDGET_PRESSED;
    for (unsigned i = 0; i < 3; i++) if (ports[i] >= 0) {
        const osg_sample *value = sample(context, ports[i]);
        if (!value || value->source_status == OSG_FEEDBACK_UNKNOWN || (value->source_status == OSG_FEEDBACK_AUTOMATIC && (!value->available || value->kind != OSG_BOOL))) return OSG_WIDGET_UNKNOWN;
    }
    for (unsigned i = 0; i < 3; i++) if (ports[i] >= 0 && !osg_widget_current_sample(context, ports[i])) return OSG_WIDGET_STALE;
    const osg_sample *confirmed = osg_widget_current_sample(context, component->confirmed_output);
    return confirmed && confirmed->kind == OSG_BOOL && confirmed->boolean && confirmed->provenance == OSG_GAME_CONFIRMED && context->outputs[component->confirmed_output].simulator ? OSG_WIDGET_ACTIVE : OSG_WIDGET_NORMAL;
}
bool osg_widget_indicator_active(const osg_widget_layer *layer, const osg_widget_state_context *context) {
    if (!layer || layer->kind != OSG_LAYER_INDICATOR) return false;
    const osg_sample *value = osg_widget_current_sample(context, layer->output);
    return value && value->kind == OSG_BOOL && value->boolean && value->provenance == OSG_GAME_CONFIRMED && context->outputs[layer->output].simulator;
}
double osg_widget_gauge_fraction(const osg_widget_layer *layer, const osg_widget_state_context *context) {
    if (!layer || layer->kind != OSG_LAYER_GAUGE || !isfinite(layer->min) || !isfinite(layer->max) || layer->max <= layer->min) return 0;
    const osg_sample *value = osg_widget_current_sample(context, layer->output);
    if (!value || value->kind != OSG_NUMBER || !isfinite(value->number)) return 0;
    return fmax(0, fmin(1, (value->number - layer->min) / (layer->max - layer->min)));
}
