#include "widget_source.h"
#include <math.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    osg_widget_arena *arena;
    osg_widget_diagnostic *error;
    const cJSON *properties, *overrides, *assets, *inputs, *outputs;
} compiler;
static const char *states[] = {"normal", "active", "pressed", "warning", "stale", "unknown", "disabled", "disconnected"};
static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static const char *str(const cJSON *v, const char *key) { return cJSON_GetStringValue(get(v, key)); }
static bool same(const char *a, const char *b) { return a && b && !strcmp(a, b); }
static double num(const cJSON *v) { return cJSON_IsNumber(v) ? v->valuedouble : NAN; }
static bool fail(compiler *c, const char *field) {
    if (c->error->code == OSG_WIDGET_SOURCE_OK) {
        c->error->code = OSG_WIDGET_SOURCE_INVALID;
        (void)snprintf(c->error->field, sizeof(c->error->field), "%s", field);
    }
    return false;
}
static bool budget(compiler *c, const char *field, size_t required, size_t available) {
    (void)fail(c, field); c->error->code = OSG_WIDGET_SOURCE_BUDGET;
    c->error->required = required; c->error->available = available; return false;
}
static bool key_in(const char *keys, const char *key) {
    if (!key) return false;
    const size_t length = strlen(key);
    for (const char *p = keys; *p;) {
        const char *end = strchr(p, '|'); if (!end) end = p + strlen(p);
        if ((size_t)(end - p) == length && !memcmp(p, key, length)) return true;
        p = *end ? end + 1 : end;
    }
    return false;
}
static bool object(compiler *c, const cJSON *v, const char *required, const char *optional, const char *field) {
    if (!cJSON_IsObject(v)) return fail(c, field);
    for (const cJSON *p = v->child; p; p = p->next) {
        if (!key_in(required, p->string) && !key_in(optional, p->string)) return fail(c, field);
        for (const cJSON *before = v->child; before != p; before = before->next)
            if (same(before->string, p->string)) return fail(c, field);
    }
    for (const char *p = required; *p;) {
        const char *end = strchr(p, '|'); if (!end) end = p + strlen(p);
        bool found = false;
        for (const cJSON *vfield = v->child; vfield; vfield = vfield->next)
            if (vfield->string && strlen(vfield->string) == (size_t)(end - p) && !memcmp(vfield->string, p, (size_t)(end - p))) found = true;
        if (!found) return fail(c, field);
        p = *end ? end + 1 : end;
    }
    return true;
}
static bool array(compiler *c, const cJSON *v, size_t min, size_t max, const char *field) {
    return (cJSON_IsArray(v) && (size_t)cJSON_GetArraySize(v) >= min && (size_t)cJSON_GetArraySize(v) <= max) || fail(c, field);
}
static bool number(compiler *c, const cJSON *v, double min, double max, bool integer, const char *field) {
    const double n = num(v);
    return (isfinite(n) && n >= min && n <= max && (!integer || floor(n) == n)) || fail(c, field);
}
static bool stable(const char *s) {
    if (!s || !*s || strlen(s) > 64) return false;
    for (size_t i = 0; s[i]; i++) if (!((s[i] >= 'a' && s[i] <= 'z') || (s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= '0' && s[i] <= '9') || (i && (s[i] == '_' || s[i] == '-')))) return false;
    return true;
}
/* Match the portable source's 120 UTF-16-unit, single-line text bound, while
 * rejecting malformed UTF-8 rather than passing it to the target renderer. */
static bool text(const char *value) {
    if (!value) return false;
    const unsigned char *s = (const unsigned char *)value; unsigned units = 0;
    while (*s) {
        uint32_t code = *s++; unsigned extra;
        if (code < 0x80) extra = 0;
        else if (code >= 0xc2 && code <= 0xdf) { code &= 0x1f; extra = 1; }
        else if (code >= 0xe0 && code <= 0xef) { code &= 0x0f; extra = 2; }
        else if (code >= 0xf0 && code <= 0xf4) { code &= 7; extra = 3; }
        else return false;
        for (unsigned i = 0; i < extra; i++) { if ((*s & 0xc0) != 0x80) return false; code = code << 6 | (*s++ & 0x3f); }
        if (code < 32 || (extra == 2 && code < 0x800) || (extra == 3 && code < 0x10000) || (code >= 0xd800 && code <= 0xdfff) || code > 0x10ffff) return false;
        units += code > 0xffff ? 2 : 1; if (units > 120) return false;
    }
    return true;
}
/* Called only after UTF-8/text validation. Match String.trim() for titles. */
static bool title_content(const char *value) {
    const unsigned char *s = (const unsigned char *)value;
    while (*s) {
        uint32_t code = *s++; unsigned extra = code < 0x80 ? 0 : code < 0xe0 ? 1 : code < 0xf0 ? 2 : 3;
        if (extra) code &= extra == 1 ? 0x1f : extra == 2 ? 0xf : 7;
        for (unsigned i = 0; i < extra; i++) code = code << 6 | (*s++ & 0x3f);
        if (code != 0x20 && code != 0xa0 && code != 0x1680 && !(code >= 0x2000 && code <= 0x200a) && code != 0x2028 && code != 0x2029 && code != 0x202f && code != 0x205f && code != 0x3000 && code != 0xfeff) return true;
    }
    return false;
}
static bool bounds(compiler *c, const cJSON *v, osg_bounds *out, const char *field) {
    if (!object(c, v, "x|y|width|height", "", field)) return false;
    double b[4]; const char *keys[] = {"x", "y", "width", "height"};
    for (unsigned i = 0; i < 4; i++) { if (!number(c, get(v, keys[i]), 0, 1, false, field)) return false; b[i] = num(get(v, keys[i])); }
    if (!b[2] || !b[3] || b[0] + b[2] > 1 + 1e-9 || b[1] + b[3] > 1 + 1e-9) return fail(c, field);
    *out = (osg_bounds){b[0], b[1], b[2], b[3]}; return true;
}
static void *allocate(compiler *c, size_t bytes) {
    osg_widget_arena *a = c->arena;
    const size_t alignment = _Alignof(max_align_t), padding = (alignment - a->used % alignment) % alignment;
    if (padding > a->capacity - a->used || bytes > a->capacity - a->used - padding) {
        (void)budget(c, "arenaBytes", a->used + padding + bytes, a->capacity); return NULL;
    }
    void *p = a->data + a->used + padding; a->used += padding + bytes; memset(p, 0, bytes); return p;
}
static const char *save(compiler *c, const char *s) {
    if (!s) return NULL;
    const size_t n = strlen(s) + 1; char *out = allocate(c, n);
    if (out) memcpy(out, s, n);
    return out;
}
static const cJSON *find(const cJSON *list, const char *key, const char *value) {
    for (const cJSON *p = list ? list->child : NULL; p; p = p->next) if (same(str(p, key), value)) return p;
    return NULL;
}
static bool color(const char *s, uint32_t *out) {
    if (!s || strlen(s) != 7 || *s != '#') return false;
    uint32_t value = 0;
    for (unsigned i = 1; i < 7; i++) {
        unsigned digit;
        if (s[i] >= '0' && s[i] <= '9') digit = (unsigned)(s[i] - '0');
        else if (s[i] >= 'a' && s[i] <= 'f') digit = (unsigned)(s[i] - 'a' + 10);
        else if (s[i] >= 'A' && s[i] <= 'F') digit = (unsigned)(s[i] - 'A' + 10);
        else return false;
        value = value << 4 | digit;
    }
    *out = value; return true;
}
static bool property_value(compiler *c, const cJSON *p, const cJSON *v, const char *field) {
    const char *type = str(p, "type"); uint32_t ignored;
    if (same(type, "number")) return number(c, v, num(get(p, "min")), num(get(p, "max")), false, field);
    if (same(type, "boolean")) return cJSON_IsBool(v) || fail(c, field);
    const char *s = cJSON_GetStringValue(v);
    if (same(type, "color")) return color(s, &ignored) || fail(c, field);
    if (!text(s)) return fail(c, field);
    if (same(type, "asset")) return !*s || find(c->assets, "id", s) || fail(c, field);
    return same(type, "text") || fail(c, field);
}
static bool properties(compiler *c) {
    if (!array(c, c->properties, 0, 24, "properties") || !cJSON_IsObject(c->overrides)) return fail(c, "properties");
    for (const cJSON *p = c->properties->child; p; p = p->next) {
        if (!object(c, p, "name|label|type|default", same(str(p, "type"), "number") ? "min|max" : "", "property") || !stable(str(p, "name")) || !text(str(p, "label"))) return fail(c, "property");
        for (const cJSON *before = c->properties->child; before != p; before = before->next) if (same(str(before, "name"), str(p, "name"))) return fail(c, "property.name");
        if (same(str(p, "type"), "number") && (!number(c, get(p, "min"), -1e9, 1e9, false, "property.min") || !number(c, get(p, "max"), -1e9, 1e9, false, "property.max") || num(get(p, "min")) >= num(get(p, "max")))) return fail(c, "property.range");
        if (!property_value(c, p, get(p, "default"), "property.default")) return false;
    }
    for (const cJSON *p = c->overrides->child; p; p = p->next) {
        const cJSON *definition = find(c->properties, "name", p->string);
        if (!definition || !property_value(c, definition, p, "property.override")) return fail(c, "property.override");
        for (const cJSON *before = c->overrides->child; before != p; before = before->next) if (same(before->string, p->string)) return fail(c, "property.override");
    }
    return true;
}
static const cJSON *resolve(compiler *c, const cJSON *v, const char *type, double min, double max, const char *field) {
    if (cJSON_IsObject(v)) {
        if (!object(c, v, "property", "", field)) return NULL;
        const char *name = str(v, "property"); const cJSON *p = find(c->properties, "name", name);
        if (!p || !same(str(p, "type"), type) || (same(type, "number") && (num(get(p, "min")) < min || num(get(p, "max")) > max))) { (void)fail(c, field); return NULL; }
        v = get(c->overrides, name); if (!v) v = get(p, "default");
    }
    bool ok; uint32_t ignored;
    if (same(type, "number")) ok = number(c, v, min, max, false, field);
    else if (same(type, "boolean")) ok = cJSON_IsBool(v);
    else if (same(type, "color")) ok = color(cJSON_GetStringValue(v), &ignored);
    else {
        const char *s = cJSON_GetStringValue(v); ok = text(s);
        if (ok && same(type, "asset")) ok = !*s || find(c->assets, "id", s);
    }
    if (!ok) { (void)fail(c, field); return NULL; }
    return v;
}
static bool paint(compiler *c, const cJSON *v, osg_widget_paint *out) {
    const char *colors[] = {"fill", "stroke"}; uint32_t *destinations[] = {&out->fill, &out->stroke};
    for (unsigned i = 0; i < 2; i++) if (get(v, colors[i])) {
        const cJSON *resolved = resolve(c, get(v, colors[i]), "color", 0, 0, colors[i]);
        if (!resolved) return false;
        (void)color(cJSON_GetStringValue(resolved), destinations[i]);
    }
    if (get(v, "strokeWidth")) {
        const cJSON *resolved = resolve(c, get(v, "strokeWidth"), "number", 0, 16, "strokeWidth");
        if (!resolved) return false;
        out->stroke_width = num(resolved); out->has_stroke_width = true;
    }
    if (get(v, "opacity")) {
        const cJSON *resolved = resolve(c, get(v, "opacity"), "number", 0, 1, "opacity");
        if (!resolved) return false;
        out->opacity = num(resolved);
    }
    if (get(v, "visible")) {
        const cJSON *resolved = resolve(c, get(v, "visible"), "boolean", 0, 1, "visible");
        if (!resolved) return false;
        out->visible = cJSON_IsTrue(resolved);
    }
    if (get(v, "text")) {
        const cJSON *resolved = resolve(c, get(v, "text"), "text", 0, 0, "text");
        if (!resolved || !(out->text = save(c, cJSON_GetStringValue(resolved)))) return false;
    }
    return true;
}
static int output_index(compiler *c, const char *name, const char *kind) {
    int i = 0;
    for (const cJSON *p = c->outputs->child; p; p = p->next, i++)
        if (same(str(p, "name"), name) && (!kind || same(str(p, "kind"), kind))) return i;
    (void)fail(c, "outputPort"); return -1;
}
static bool ports(compiler *c, osg_widget_component *out) {
    if (!array(c, c->inputs, 0, 8, "inputs") || !array(c, c->outputs, 0, 8, "outputs")) return false;
    const cJSON *lists[] = {c->inputs, c->outputs};
    for (unsigned list = 0; list < 2; list++) for (const cJSON *p = lists[list]->child; p; p = p->next) {
        const char *kind = str(p, "kind"), *name = str(p, "name");
        const char *optional = list ? "unit|range" : same(kind, "axis") ? "axisMode|fallback" : same(kind, "hat") ? "fallback" : "";
        if (!object(c, p, "name|label|kind", optional, "port") || !stable(name) || !text(str(p, "label"))) return fail(c, "port");
        for (unsigned prior_list = 0; prior_list <= list; prior_list++) for (const cJSON *before = lists[prior_list]->child; before && before != p; before = before->next)
            if (same(str(before, "name"), name)) return fail(c, "port.name");
        if (!list) {
            if (same(kind, "axis")) {
                const char *mode = str(p, "axisMode");
                if ((!same(mode, "unipolar") && !same(mode, "bipolar")) || !number(c, get(p, "fallback"), same(mode, "bipolar") ? -1 : 0, 1, false, "input.fallback")) return fail(c, "input.axisMode");
                out->bipolar = same(mode, "bipolar"); out->fallback = num(get(p, "fallback"));
            } else if (same(kind, "hat")) { if (num(get(p, "fallback")) != -1) return fail(c, "input.fallback"); }
            else if (!same(kind, "button")) return fail(c, "input.kind");
        } else {
            osg_widget_port *port = &out->outputs[out->output_count++];
            if (same(kind, "indicator")) port->kind = OSG_BOOL;
            else if (same(kind, "text")) port->kind = OSG_STRING;
            else if (same(kind, "gauge")) port->kind = OSG_NUMBER;
            else return fail(c, "output.kind");
            if (!(port->name = save(c, name))) return false;
            if (get(p, "unit")) { if (!text(str(p, "unit"))) return fail(c, "output.unit"); port->has_unit = true; if (!(port->unit = save(c, str(p, "unit")))) return false; }
            const cJSON *range = get(p, "range");
            if (range) {
                if (port->kind != OSG_NUMBER || !object(c, range, "min|max", "", "output.range") || !number(c, get(range, "min"), -1e9, 1e9, false, "output.min") || !number(c, get(range, "max"), -1e9, 1e9, false, "output.max") || num(get(range, "min")) >= num(get(range, "max"))) return fail(c, "output.range");
                port->has_range = true; port->min = num(get(range, "min")); port->max = num(get(range, "max"));
            }
        }
    }
    return true;
}
static bool interaction(compiler *c, const cJSON *v, osg_widget_component *out) {
    const char *kind = str(v, "kind"), *optional;
    if (same(kind, "none")) { out->interaction = OSG_INTERACTION_NONE; optional = ""; }
    else if (same(kind, "momentary")) { out->interaction = OSG_INTERACTION_MOMENTARY; optional = "port"; }
    else if (same(kind, "toggle")) { out->interaction = OSG_INTERACTION_TOGGLE; optional = "port"; }
    else if (same(kind, "selector")) { out->interaction = OSG_INTERACTION_SELECTOR; optional = "port|options"; }
    else if (same(kind, "navigate")) { out->interaction = OSG_INTERACTION_NAVIGATE; optional = "pageProperty"; }
    else return fail(c, "interaction.kind");
    if (!object(c, v, "kind", optional, "interaction")) return false;
    if (out->interaction == OSG_INTERACTION_NONE || out->interaction == OSG_INTERACTION_NAVIGATE) {
        if (cJSON_GetArraySize(c->inputs)) return fail(c, "interaction.inputs");
        if (out->interaction == OSG_INTERACTION_NAVIGATE) {
            const char *name = str(v, "pageProperty"); const cJSON *p = find(c->properties, "name", name);
            if (!p || !same(str(p, "type"), "text")) return fail(c, "interaction.pageProperty");
            const cJSON *value = get(c->overrides, name); if (!value) value = get(p, "default");
            if (!(out->navigate_page = save(c, cJSON_GetStringValue(value)))) return false;
        }
        return true;
    }
    const char *name = str(v, "port"); const cJSON *port = find(c->inputs, "name", name);
    if (cJSON_GetArraySize(c->inputs) != 1 || !port || !same(str(port, "kind"), out->interaction == OSG_INTERACTION_SELECTOR ? "axis" : "button")) return fail(c, "interaction.port");
    if (!(out->input_name = save(c, name))) return false;
    if (out->interaction == OSG_INTERACTION_SELECTOR) {
        const cJSON *options = get(v, "options"); bool fallback = false;
        if (!array(c, options, 2, 16, "interaction.options")) return false;
        for (const cJSON *p = options->child; p; p = p->next) {
            if (!object(c, p, "label|value", "", "option") || !text(str(p, "label")) || !number(c, get(p, "value"), out->bipolar ? -1 : 0, 1, false, "option.value")) return fail(c, "option");
            const double value = num(get(p, "value"));
            for (unsigned i = 0; i < out->option_count; i++) if (out->options[i] == value) return fail(c, "option.value");
            if (!(out->option_labels[out->option_count] = save(c, str(p, "label")))) return false;
            out->options[out->option_count++] = value; if (value == out->fallback) fallback = true;
        }
        if (!fallback) return fail(c, "interaction.fallback");
    }
    return true;
}
static bool state(compiler *c, const cJSON *v, osg_widget_component *out) {
    if (!object(c, v, "", "confirmedPort|warningPort|enabledPort", "state")) return false;
    const char *keys[] = {"confirmedPort", "warningPort", "enabledPort"};
    int8_t *destinations[] = {&out->confirmed_output, &out->warning_output, &out->enabled_output};
    for (unsigned i = 0; i < 3; i++) {
        *destinations[i] = -1;
        if (get(v, keys[i])) { const int index = output_index(c, str(v, keys[i]), "indicator"); if (index < 0) return false; *destinations[i] = (int8_t)index; }
    }
    return true;
}
static bool layer(compiler *c, const cJSON *v, osg_widget_layer *out) {
    const char *kind = str(v, "kind"), *extra = NULL;
    const char *kinds[] = {"rectangle", "ellipse", "line", "text", "image", "indicator", "gauge"};
    const char *extras[] = {"radius", "", "", "text|font|fontSize|align|outputPort|precision|unit", "assetId", "outputPort", "outputPort|min|max|orientation"};
    for (unsigned i = 0; i < 7; i++) if (same(kind, kinds[i])) { out->kind = (osg_layer_kind)i; extra = extras[i]; }
    if (!extra) return fail(c, "layer.kind");
    char optional[160]; (void)snprintf(optional, sizeof(optional), "fill|stroke|strokeWidth|opacity|visible|states|%s", extra);
    if (!object(c, v, "id|label|kind|bounds", optional, "layer") || !stable(str(v, "id")) || !text(str(v, "label")) || !bounds(c, get(v, "bounds"), &out->bounds, "layer.bounds")) return fail(c, "layer");
    if (!(out->id = save(c, str(v, "id")))) return false;
    out->output = -1;
    osg_widget_paint base = {.fill = OSG_WIDGET_SOURCE_NO_COLOR, .stroke = OSG_WIDGET_SOURCE_NO_COLOR, .opacity = 1, .visible = true};
    if (!paint(c, v, &base)) return false;
    if (out->kind == OSG_LAYER_RECTANGLE && get(v, "radius")) {
        const cJSON *value = resolve(c, get(v, "radius"), "number", 0, 64, "radius"); if (!value) return false; out->radius = num(value);
    }
    if (out->kind == OSG_LAYER_TEXT) {
        if (!base.text || !number(c, get(v, "fontSize"), 8, 128, true, "fontSize")) return fail(c, "text");
        const char *font = str(v, "font"), *align = str(v, "align");
        if (!same(font, "sans") && !same(font, "mono")) return fail(c, "font");
        if (!same(align, "left") && !same(align, "center") && !same(align, "right")) return fail(c, "align");
        out->font = same(font, "mono"); out->align = same(align, "center") ? 1 : same(align, "right") ? 2 : 0; out->font_size = (uint8_t)num(get(v, "fontSize"));
        if (get(v, "precision")) { if (!number(c, get(v, "precision"), 0, 4, true, "precision")) return false; out->precision = (uint8_t)num(get(v, "precision")); }
        if (get(v, "unit")) { if (!text(str(v, "unit"))) return fail(c, "unit"); if (!(out->unit = save(c, str(v, "unit")))) return false; }
    }
    if (get(v, "outputPort")) { const int index = output_index(c, str(v, "outputPort"), out->kind == OSG_LAYER_INDICATOR ? "indicator" : out->kind == OSG_LAYER_GAUGE ? "gauge" : NULL); if (index < 0) return false; out->output = (int8_t)index; }
    if ((out->kind == OSG_LAYER_INDICATOR || out->kind == OSG_LAYER_GAUGE) && out->output < 0) return fail(c, "outputPort");
    if (out->kind == OSG_LAYER_IMAGE) {
        const cJSON *value = resolve(c, get(v, "assetId"), "asset", 0, 0, "assetId");
        if (!value || !(out->asset_id = save(c, cJSON_GetStringValue(value)))) return false;
    }
    if (out->kind == OSG_LAYER_GAUGE) {
        const cJSON *low = resolve(c, get(v, "min"), "number", -1e9, 1e9, "min"), *high = resolve(c, get(v, "max"), "number", -1e9, 1e9, "max");
        if (!low || !high || num(low) >= num(high)) return fail(c, "gauge.range");
        const cJSON *low_source = get(v, "min"), *high_source = get(v, "max");
        const cJSON *low_default = cJSON_IsObject(low_source) ? get(find(c->properties, "name", str(low_source, "property")), "default") : low_source;
        const cJSON *high_default = cJSON_IsObject(high_source) ? get(find(c->properties, "name", str(high_source, "property")), "default") : high_source;
        if (num(low_default) >= num(high_default)) return fail(c, "gauge.defaultRange");
        out->min = num(low); out->max = num(high); const char *orientation = str(v, "orientation");
        if (!same(orientation, "horizontal") && !same(orientation, "vertical") && !same(orientation, "radial")) return fail(c, "orientation");
        out->orientation = same(orientation, "vertical") ? 1 : same(orientation, "radial") ? 2 : 0;
    }
    const cJSON *overrides = get(v, "states");
    if (overrides && !object(c, overrides, "", "normal|active|pressed|warning|stale|unknown|disabled|disconnected", "states")) return false;
    for (const cJSON *p = overrides ? overrides->child : NULL; p; p = p->next)
        if (!object(c, p, "", out->kind == OSG_LAYER_TEXT ? "fill|stroke|strokeWidth|opacity|visible|text" : "fill|stroke|strokeWidth|opacity|visible", "state.paint")) return false;
    if (!paint(c, get(overrides, "normal"), &base)) return false;
    for (unsigned i = 0; i < OSG_WIDGET_SOURCE_STATES; i++) {
        out->paints[i] = base;
        if (i && !paint(c, get(overrides, states[i]), &out->paints[i])) return false;
    }
    return true;
}
bool osg_widget_source_compile(const cJSON *d, const cJSON *overrides, const cJSON *assets, osg_widget_arena *arena, const osg_widget_component **result, osg_widget_diagnostic *diagnostic) {
    osg_widget_diagnostic local = {0}; if (!diagnostic) diagnostic = &local; memset(diagnostic, 0, sizeof(*diagnostic));
    if (result) *result = NULL;
    compiler c = {.arena = arena, .error = diagnostic, .overrides = overrides, .assets = assets};
    if (!result || !arena || !arena->data || (uintptr_t)arena->data % _Alignof(max_align_t) || arena->used > arena->capacity || !cJSON_IsArray(assets)) return fail(&c, "arguments");
    const size_t mark = arena->used, layer_mark = arena->layers, component_mark = arena->components;
    bool ok = false;
    const char *id = str(d, "id"); if (stable(id)) (void)snprintf(diagnostic->definition, sizeof(diagnostic->definition), "%s", id);
    if (!object(&c, d, "formatVersion|id|version|title|size|properties|inputs|outputs|interaction|hitArea|layers|state", "", "definition") || num(get(d, "formatVersion")) != 1 || !stable(id) || !text(str(d, "title")) || !title_content(str(d, "title")) || !number(&c, get(d, "version"), 1, 65535, true, "version")) { (void)fail(&c, "definition"); goto done; }
    if (arena->components >= arena->max_components) { (void)budget(&c, "components", arena->components + 1, arena->max_components); goto done; }
    c.properties = get(d, "properties"); c.inputs = get(d, "inputs"); c.outputs = get(d, "outputs");
    if (!properties(&c)) goto done;
    osg_widget_component *out = allocate(&c, sizeof(*out)); if (!out) goto done;
    const cJSON *size = get(d, "size"), *layers = get(d, "layers");
    if (!object(&c, size, "width|height", "", "size") || !number(&c, get(size, "width"), 32, 1024, true, "size.width") || !number(&c, get(size, "height"), 32, 1024, true, "size.height") || !bounds(&c, get(d, "hitArea"), &out->hit_area, "hitArea") || !array(&c, layers, 1, 32, "layers")) goto done;
    out->width = (uint16_t)num(get(size, "width")); out->height = (uint16_t)num(get(size, "height")); out->version = (uint16_t)num(get(d, "version")); out->layer_count = (uint8_t)cJSON_GetArraySize(layers);
    if (out->layer_count > arena->max_layers || arena->layers > arena->max_layers - out->layer_count) { (void)budget(&c, "layers", arena->layers + out->layer_count, arena->max_layers); goto done; }
    if (!(out->id = save(&c, id)) || !(out->title = save(&c, str(d, "title"))) || !ports(&c, out) || !interaction(&c, get(d, "interaction"), out) || !state(&c, get(d, "state"), out)) goto done;
    out->layers = allocate(&c, sizeof(*out->layers) * out->layer_count); if (!out->layers) goto done;
    unsigned i = 0;
    for (const cJSON *v = layers->child; v; v = v->next, i++) {
        if (stable(str(v, "id"))) (void)snprintf(diagnostic->layer, sizeof(diagnostic->layer), "%s", str(v, "id"));
        for (const cJSON *before = layers->child; before != v; before = before->next) if (same(str(before, "id"), str(v, "id"))) { (void)fail(&c, "layer.id"); goto done; }
        if (!layer(&c, v, &out->layers[i])) goto done;
    }
    arena->layers += out->layer_count; arena->components++; *result = out; memset(diagnostic, 0, sizeof(*diagnostic)); ok = true;
done:
    if (!ok) { arena->used = mark; arena->layers = layer_mark; arena->components = component_mark; }
    return ok;
}
