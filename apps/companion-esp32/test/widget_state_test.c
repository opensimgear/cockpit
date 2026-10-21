#include "widget_state.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static double n(const cJSON *v) { assert(cJSON_IsNumber(v)); return v->valuedouble; }
static cJSON *load(const char *path) {
    FILE *f = fopen(path, "rb"); assert(f); assert(!fseek(f, 0, SEEK_END)); const long size = ftell(f); assert(size > 0 && size < 8 * 1024 * 1024); rewind(f);
    char *bytes = malloc((size_t)size + 1); assert(bytes && fread(bytes, 1, (size_t)size, f) == (size_t)size); bytes[size] = 0; fclose(f);
    cJSON *result = cJSON_Parse(bytes); free(bytes); assert(result); return result;
}
static osg_widget_state_context context(const cJSON *frame, osg_sample storage[8]) {
    osg_widget_state_context out = {.now_ms = (uint64_t)n(get(frame, "now")), .connected = cJSON_IsTrue(get(frame, "connected")), .enabled = cJSON_IsTrue(get(frame, "enabled")), .pressed = cJSON_IsTrue(get(frame, "pressed"))};
    const cJSON *specs = get(frame, "specs"); assert(cJSON_GetArraySize(specs) <= 8);
    for (int i = 0; i < cJSON_GetArraySize(specs); i++) {
        const cJSON *spec = cJSON_GetArrayItem(specs, i); if (cJSON_IsNull(spec)) continue;
        assert(cJSON_GetArraySize(spec) == 6);
        osg_sample *s = &storage[i]; const unsigned status = (unsigned)n(cJSON_GetArrayItem(spec, 0));
        switch (status) {
            case 0: s->source_status = OSG_FEEDBACK_AUTOMATIC; break;
            case 1: s->source_status = OSG_FEEDBACK_UNKNOWN; break;
            case 2: s->source_status = OSG_FEEDBACK_STALE; break;
            case 3: s->source_status = OSG_FEEDBACK_DISCONNECTED; break;
            default: abort();
        }
        const cJSON *value = cJSON_GetArrayItem(spec, 1); s->available = !cJSON_IsNull(value);
        if (cJSON_IsBool(value)) { s->kind = OSG_BOOL; s->boolean = cJSON_IsTrue(value); }
        else if (cJSON_IsNumber(value)) { s->kind = OSG_NUMBER; s->number = n(value); }
        else if (cJSON_IsString(value)) { s->kind = OSG_STRING; assert(strlen(value->valuestring) < sizeof(s->text)); strcpy(s->text, value->valuestring); }
        s->provenance = (osg_provenance)n(cJSON_GetArrayItem(spec, 2)); s->received_ms = (uint64_t)n(cJSON_GetArrayItem(spec, 4)); s->source_age_ms = (uint32_t)n(cJSON_GetArrayItem(spec, 5));
        out.outputs[i] = (osg_widget_sample_view){s, n(cJSON_GetArrayItem(spec, 3)) == 2};
    }
    return out;
}
int main(int argc, char **argv) {
    assert(argc == 3); cJSON *source = load(argv[1]), *frames = load(argv[2]);
    unsigned char *memory = malloc(128 * 1024); assert(memory);
    osg_widget_arena arena = {.data = memory, .capacity = 128 * 1024, .max_layers = 256, .max_components = 32};
    const osg_widget_component *components[32] = {0}; unsigned count = 0;
    for (const cJSON *c = get(source, "cases")->child; c; c = c->next) {
        assert(count < 32 && osg_widget_source_compile(get(c, "definition"), get(c, "properties"), get(source, "assets"), &arena, &components[count], NULL)); count++;
    }
    cJSON_Delete(source);
    const char *states[] = {"normal", "active", "pressed", "warning", "stale", "unknown", "disabled", "disconnected"}; unsigned total = 0;
    for (const cJSON *frame = get(frames, "cases")->child; frame; frame = frame->next, total++) {
        const unsigned index = (unsigned)n(get(frame, "index")); assert(index < count); const osg_widget_component *component = components[index];
        osg_sample storage[8] = {0}; osg_widget_state_context ctx = context(frame, storage);
        const osg_widget_state actual = osg_widget_state_resolve(component, &ctx); const char *expected = cJSON_GetStringValue(get(frame, "state"));
        if (strcmp(states[actual], expected)) { fprintf(stderr, "Frame %u: state=%s expected=%s\n", total, states[actual], expected); abort(); }
        assert(osg_widget_gate_open(component, &ctx) == cJSON_IsTrue(get(frame, "gate")));
        for (unsigned i = 0; i < component->layer_count; i++) {
            const osg_widget_layer *layer = &component->layers[i];
            assert(osg_widget_indicator_active(layer, &ctx) == cJSON_IsTrue(cJSON_GetArrayItem(get(frame, "indicators"), (int)i)));
            assert(fabs(osg_widget_gauge_fraction(layer, &ctx) - n(cJSON_GetArrayItem(get(frame, "gauges"), (int)i))) < 1e-12);
        }
    }
    const osg_widget_component *component = components[11];
    osg_sample active = {.available = true, .kind = OSG_BOOL, .boolean = true, .provenance = OSG_GAME_CONFIRMED, .received_ms = UINT64_MAX - 1};
    osg_widget_state_context ctx = {.now_ms = UINT64_MAX, .connected = true, .enabled = true};
    ctx.outputs[0] = (osg_widget_sample_view){&active, true};
    assert(osg_widget_indicator_active(&component->layers[5], &ctx));
    active.source_age_ms = UINT32_MAX; assert(!osg_widget_indicator_active(&component->layers[5], &ctx));
    assert(!osg_widget_gate_open(NULL, &ctx) && !osg_widget_gate_open(component, NULL));
    assert(osg_widget_state_resolve(NULL, &ctx) == OSG_WIDGET_DISCONNECTED && osg_widget_state_resolve(component, NULL) == OSG_WIDGET_DISCONNECTED);
    assert(!osg_widget_indicator_active(NULL, &ctx) && !osg_widget_indicator_active(&component->layers[5], NULL));
    assert(osg_widget_gauge_fraction(NULL, &ctx) == 0 && osg_widget_gauge_fraction(&component->layers[6], NULL) == 0);
    osg_widget_layer gauge = component->layers[6]; osg_sample number = {.available = true, .kind = OSG_NUMBER, .received_ms = 0};
    ctx.now_ms = 0; ctx.outputs[gauge.output] = (osg_widget_sample_view){&number, false};
    number.number = NAN; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    number.number = INFINITY; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    gauge.min = NAN; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    gauge.min = 1; gauge.max = INFINITY; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    gauge.max = 1; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    gauge.output = 8; assert(osg_widget_gauge_fraction(&gauge, &ctx) == 0);
    cJSON_Delete(frames); free(memory);
    printf("Shared state, enable gate, confirmed indicator and gauge projections: %u passed\n", total); return 0;
}
