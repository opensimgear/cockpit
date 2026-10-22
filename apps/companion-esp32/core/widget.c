#include "widget.h"
#include <string.h>

bool osg_widget_id_valid(const char *s) {
    if (!s || !*s || strlen(s) > 64) return false;
    for (size_t i = 0; s[i]; i++) if (!((s[i] >= 'a' && s[i] <= 'z') || (s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= '0' && s[i] <= '9') || (i && (s[i] == '_' || s[i] == '-')))) return false;
    return true;
}
static bool text_scan(const char *value, unsigned maximum, size_t *prefix_bytes) {
    if (!value) return false;
    const unsigned char *s = (const unsigned char *)value; unsigned units = 0; size_t prefix = 0;
    while (*s) {
        uint32_t code = *s++; unsigned extra;
        if (code < 0x80) extra = 0;
        else if (code >= 0xc2 && code <= 0xdf) { code &= 0x1f; extra = 1; }
        else if (code >= 0xe0 && code <= 0xef) { code &= 0x0f; extra = 2; }
        else if (code >= 0xf0 && code <= 0xf4) { code &= 7; extra = 3; }
        else return false;
        for (unsigned i = 0; i < extra; i++) { if ((*s & 0xc0) != 0x80) return false; code = code << 6 | (*s++ & 0x3f); }
        if (code < 32 || (extra == 2 && code < 0x800) || (extra == 3 && code < 0x10000) || (code >= 0xd800 && code <= 0xdfff) || code > 0x10ffff) return false;
        units += code > 0xffff ? 2 : 1; if (units > maximum) return false;
        if (units <= 120) prefix = (size_t)(s - (const unsigned char *)value);
    }
    if (prefix_bytes) *prefix_bytes = prefix;
    return true;
}
bool osg_widget_text_valid(const char *value) { return text_scan(value, 120, NULL); }
bool osg_widget_feedback_text(const char *value, char *out, size_t capacity) {
    size_t prefix;
    if (!out || !text_scan(value, 256, &prefix) || prefix >= capacity) return false;
    memcpy(out, value, prefix); out[prefix] = 0; return true;
}
bool osg_widget_input(const osg_widget *w, osg_value_kind *kind, double *fallback) {
    if (!w || !kind || !fallback) return false;
    *kind = OSG_BOOL; *fallback = 0;
    if (w->kind == OSG_BUTTON) return true;
    if (w->kind != OSG_COMPONENT || !w->component || !w->component->definition) return false;
    const osg_widget_component *d = w->component->definition;
    if (d->interaction == OSG_INTERACTION_SELECTOR) { *kind = OSG_NUMBER; *fallback = d->fallback; return true; }
    return d->interaction == OSG_INTERACTION_MOMENTARY || d->interaction == OSG_INTERACTION_TOGGLE;
}
bool osg_widget_text_blank(const char *value) {
    if (!osg_widget_text_valid(value)) return false;
    const unsigned char *s = (const unsigned char *)value;
    while (*s) {
        uint32_t code = *s++; unsigned extra = code < 0x80 ? 0 : code < 0xe0 ? 1 : code < 0xf0 ? 2 : 3;
        if (extra) code &= extra == 1 ? 0x1f : extra == 2 ? 0xf : 7;
        for (unsigned i = 0; i < extra; i++) code = code << 6 | (*s++ & 0x3f);
        if (code != 0x20 && code != 0xa0 && code != 0x1680 && !(code >= 0x2000 && code <= 0x200a) && code != 0x2028 && code != 0x2029 && code != 0x202f && code != 0x205f && code != 0x3000 && code != 0xfeff) return false;
    }
    return true;
}
bool osg_panel_output(const osg_panel *panel, size_t slot, osg_output_contract *out) {
    if (!panel || !out || slot >= OSG_MAX_WIDGETS) return false;
    for (size_t i = 0; i < panel->widget_count; i++) {
        const osg_widget *w = &panel->widgets[i];
        if (w->kind == OSG_COMPONENT) {
            const osg_widget_binding *binding = w->component;
            if (!binding || !binding->definition) continue;
            for (unsigned p = 0; p < binding->definition->output_count; p++) if (binding->outputs[p].slot == slot) {
                const osg_widget_port *port = &binding->definition->outputs[p];
                *out = (osg_output_contract){binding->outputs[p].id, binding->outputs[p].channel, port->unit, port->kind, port->has_unit, port->has_range, true, port->min, port->max}; return true;
            }
        } else if (i == slot && w->kind != OSG_BUTTON && w->kind != OSG_IMAGE) {
            const bool numeric = w->kind == OSG_NUMERIC || w->kind == OSG_GAUGE;
            *out = (osg_output_contract){w->output_id, w->reference, w->unit, w->kind == OSG_INDICATOR ? OSG_BOOL : w->kind == OSG_TEXT ? OSG_STRING : OSG_NUMBER, numeric, numeric, false, w->min, w->max}; return true;
        }
    }
    return false;
}
