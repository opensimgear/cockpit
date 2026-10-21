#include "widget_source.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static const char *str(const cJSON *v, const char *key) { return cJSON_GetStringValue(get(v, key)); }
static double num(const cJSON *v, const char *key, double fallback) { const cJSON *p = get(v, key); return cJSON_IsNumber(p) ? p->valuedouble : fallback; }
static void equal(const char *actual, const char *expected) { assert(actual && expected && !strcmp(actual, expected)); }
static uint32_t rgb(const char *s) { unsigned out; if (!s) return OSG_WIDGET_SOURCE_NO_COLOR; assert(sscanf(s, "#%x", &out) == 1); return out; }
static int output_index(const osg_widget_component *v, const char *name) {
    for (unsigned i = 0; name && i < v->output_count; i++) if (!strcmp(v->outputs[i].name, name)) return (int)i;
    return -1;
}
static void compare_bounds(osg_bounds actual, const cJSON *expected) {
    assert(actual.x == num(expected, "x", -1) && actual.y == num(expected, "y", -1));
    assert(actual.width == num(expected, "width", -1) && actual.height == num(expected, "height", -1));
}
static void compare(const osg_widget_component *v, const cJSON *source) {
    const cJSON *d = get(source, "definition"), *size = get(d, "size"), *interaction = get(d, "interaction"), *state = get(d, "state");
    equal(v->id, str(d, "id")); equal(v->title, str(d, "title"));
    assert(v->version == num(d, "version", 0) && v->width == num(size, "width", 0) && v->height == num(size, "height", 0));
    compare_bounds(v->hit_area, get(d, "hitArea"));
    const char *interactions[] = {"none", "momentary", "toggle", "selector", "navigate"};
    equal(interactions[v->interaction], str(interaction, "kind"));
    assert(v->confirmed_output == output_index(v, str(state, "confirmedPort")));
    assert(v->warning_output == output_index(v, str(state, "warningPort")));
    assert(v->enabled_output == output_index(v, str(state, "enabledPort")));
    if (v->interaction == OSG_INTERACTION_SELECTOR) {
        const cJSON *port = cJSON_GetArrayItem(get(d, "inputs"), 0), *options = get(interaction, "options");
        assert(v->bipolar == !strcmp(str(port, "axisMode"), "bipolar") && v->fallback == num(port, "fallback", 99));
        assert(v->option_count == cJSON_GetArraySize(options));
        for (unsigned i = 0; i < v->option_count; i++) { const cJSON *option = cJSON_GetArrayItem(options, (int)i); equal(v->option_labels[i], str(option, "label")); assert(v->options[i] == num(option, "value", 99)); }
    }
    if (v->input_name) equal(v->input_name, str(interaction, "port"));
    assert(v->output_count == cJSON_GetArraySize(get(d, "outputs")));
    for (unsigned i = 0; i < v->output_count; i++) {
        const osg_widget_port *p = &v->outputs[i]; const cJSON *expected = cJSON_GetArrayItem(get(d, "outputs"), (int)i);
        equal(p->name, str(expected, "name")); const char *kinds[] = {"indicator", "gauge", "text"}; equal(kinds[p->kind], str(expected, "kind"));
        assert(p->has_unit == (get(expected, "unit") != NULL) && p->has_range == (get(expected, "range") != NULL));
        if (p->has_unit) equal(p->unit, str(expected, "unit"));
        if (p->has_range) { assert(p->min == num(get(expected, "range"), "min", NAN)); assert(p->max == num(get(expected, "range"), "max", NAN)); }
    }
    for (unsigned state_index = 0; state_index < OSG_WIDGET_SOURCE_STATES; state_index++) {
        const cJSON *layers = cJSON_GetArrayItem(get(source, "resolved"), (int)state_index);
        assert(v->layer_count == cJSON_GetArraySize(layers));
        for (unsigned i = 0; i < v->layer_count; i++) {
            const osg_widget_layer *l = &v->layers[i]; const osg_widget_paint *p = &l->paints[state_index]; const cJSON *expected = cJSON_GetArrayItem(layers, (int)i);
            const char *kinds[] = {"rectangle", "ellipse", "line", "text", "image", "indicator", "gauge"};
            equal(l->id, str(expected, "id")); equal(kinds[l->kind], str(expected, "kind")); compare_bounds(l->bounds, get(expected, "bounds"));
            assert(p->fill == rgb(str(expected, "fill")) && p->stroke == rgb(str(expected, "stroke")));
            assert(p->stroke_width == num(expected, "strokeWidth", 0) && p->has_stroke_width == (get(expected, "strokeWidth") != NULL));
            assert(p->opacity == num(expected, "opacity", 1) && p->visible == !cJSON_IsFalse(get(expected, "visible")));
            assert(l->output == output_index(v, str(expected, "outputPort")));
            if (l->kind == OSG_LAYER_TEXT) {
                equal(p->text, str(expected, "text")); const char *fonts[] = {"sans", "mono"}, *aligns[] = {"left", "center", "right"};
                equal(fonts[l->font], str(expected, "font")); equal(aligns[l->align], str(expected, "align"));
                assert(l->font_size == num(expected, "fontSize", 0) && l->precision == num(expected, "precision", 0));
                if (get(expected, "unit")) equal(l->unit, str(expected, "unit"));
            } else if (l->kind == OSG_LAYER_RECTANGLE) assert(l->radius == num(expected, "radius", 0));
            else if (l->kind == OSG_LAYER_IMAGE) equal(l->asset_id, str(expected, "assetId"));
            else if (l->kind == OSG_LAYER_GAUGE) {
                const char *orientations[] = {"horizontal", "vertical", "radial"}; equal(orientations[l->orientation], str(expected, "orientation"));
                assert(l->min == num(expected, "min", NAN) && l->max == num(expected, "max", NAN));
            }
        }
    }
}
static cJSON *load(const char *path) {
    FILE *f = fopen(path, "rb"); assert(f); assert(!fseek(f, 0, SEEK_END)); const long size = ftell(f); assert(size > 0 && size < 8 * 1024 * 1024); rewind(f);
    char *bytes = malloc((size_t)size + 1); assert(bytes && fread(bytes, 1, (size_t)size, f) == (size_t)size); bytes[size] = 0; fclose(f);
    cJSON *result = cJSON_Parse(bytes); free(bytes); assert(result); return result;
}
static cJSON *mutate(cJSON *copy, const cJSON *mutation) {
    cJSON *parent = cJSON_GetObjectItemCaseSensitive(copy, str(mutation, "root")); const cJSON *path = get(mutation, "path");
    const int count = cJSON_GetArraySize(path); assert(count > 0);
    for (int i = 0; i < count - 1; i++) { const cJSON *key = cJSON_GetArrayItem(path, i); parent = cJSON_IsString(key) ? cJSON_GetObjectItemCaseSensitive(parent, key->valuestring) : cJSON_GetArrayItem(parent, key->valueint); assert(parent); }
    const cJSON *key = cJSON_GetArrayItem(path, count - 1);
    if (cJSON_IsTrue(get(mutation, "remove"))) { assert(cJSON_IsString(key)); cJSON_DeleteItemFromObjectCaseSensitive(parent, key->valuestring); }
    else {
        cJSON *value = cJSON_Duplicate(get(mutation, "value"), true); assert(value);
        if (cJSON_IsString(key)) {
            if (get(parent, key->valuestring)) assert(cJSON_ReplaceItemInObjectCaseSensitive(parent, key->valuestring, value));
            else assert(cJSON_AddItemToObject(parent, key->valuestring, value));
        } else assert(cJSON_ReplaceItemInArray(parent, key->valueint, value));
    }
    return copy;
}
static void adversarial(const cJSON *fixture, unsigned char *memory, size_t capacity) {
    const cJSON *cases = get(fixture, "cases"), *assets = get(fixture, "assets");
    unsigned index = 0;
    for (const cJSON *m = get(fixture, "mutations")->child; m; m = m->next, index++) {
        cJSON *source = cJSON_Duplicate(cJSON_GetArrayItem(cases, (int)num(m, "index", -1)), true); assert(source); mutate(source, m);
        osg_widget_arena arena = {.data = memory, .capacity = capacity, .max_layers = 128, .max_components = 32};
        const osg_widget_component *result; osg_widget_diagnostic diagnostic;
        const bool actual = osg_widget_source_compile(get(source, "definition"), get(source, "properties"), assets, &arena, &result, &diagnostic), expected = cJSON_IsTrue(get(m, "valid"));
        if (actual != expected) { char *json = cJSON_PrintUnformatted(m); fprintf(stderr, "Mutation %u differs: actual=%d expected=%d field=%s %s\n", index, actual, expected, diagnostic.field, json); free(json); abort(); }
        if (!actual) { assert(!result && !arena.used && !arena.layers && !arena.components); assert(diagnostic.code != OSG_WIDGET_SOURCE_OK && diagnostic.field[0]); }
        cJSON_Delete(source);
    }
    printf("Differential source mutations: %u\n", index);
}
static void boundaries(const cJSON *fixture, unsigned char *memory, size_t capacity) {
    const cJSON *source = cJSON_GetArrayItem(get(fixture, "cases"), 0), *d = get(source, "definition"), *props = get(source, "properties"), *assets = get(fixture, "assets");
    osg_widget_arena arena = {.data = memory, .capacity = capacity, .max_layers = 128, .max_components = 32};
    const osg_widget_component *first, *result; osg_widget_diagnostic e;
    assert(osg_widget_source_compile(d, props, assets, &arena, &first, &e));
    const size_t used = arena.used, layers = arena.layers; unsigned char *snapshot = malloc(used); assert(snapshot); memcpy(snapshot, memory, used);
    for (unsigned failure = 0; failure < 3; failure++) {
        arena.capacity = failure == 0 ? used : capacity; arena.max_layers = failure == 1 ? layers : 128; arena.max_components = failure == 2 ? 1 : 32;
        assert(!osg_widget_source_compile(d, props, assets, &arena, &result, &e));
        assert(!result && arena.used == used && arena.layers == layers && arena.components == 1 && e.code == OSG_WIDGET_SOURCE_BUDGET && e.required > e.available);
        assert(!memcmp(snapshot, memory, used)); equal(first->layers[1].paints[OSG_WIDGET_NORMAL].text, "ENGINE");
    }
    arena.capacity = capacity; arena.max_layers = 128; arena.max_components = 32;
    cJSON *bad = cJSON_Duplicate(d, true); assert(bad);
    const char *invalid[] = {"\xc0\x80", "\xed\xa0\x80", "\xf4\x90\x80\x80", "\xf0\x80\x80\x80", "\xe0\x80\x80", "\xe2\x82", "a\n", "\x80"};
    for (unsigned i = 0; i < sizeof(invalid) / sizeof(invalid[0]); i++) {
        assert(cJSON_ReplaceItemInObjectCaseSensitive(bad, "title", cJSON_CreateString(invalid[i])));
        assert(!osg_widget_source_compile(bad, props, assets, &arena, &result, &e)); assert(!memcmp(snapshot, memory, used));
    }
    assert(cJSON_ReplaceItemInObjectCaseSensitive(bad, "title", cJSON_CreateString("Valid")));
    assert(cJSON_AddStringToObject(bad, "id", "duplicate")); assert(!osg_widget_source_compile(bad, props, assets, &arena, &result, &e)); cJSON_Delete(bad);
    cJSON *override = cJSON_CreateObject(); assert(cJSON_AddStringToObject(override, "label", "SECOND")); assert(cJSON_AddStringToObject(override, "label", "DUPLICATE"));
    assert(!osg_widget_source_compile(d, override, assets, &arena, &result, &e)); cJSON_Delete(override);
    arena.data = memory + 1; assert(!osg_widget_source_compile(d, props, assets, &arena, &result, &e)); arena.data = memory;
    arena.used = capacity + 1; assert(!osg_widget_source_compile(d, props, assets, &arena, &result, &e)); arena.used = used;
    assert(!osg_widget_source_compile(d, props, assets, NULL, &result, &e));
    assert(!osg_widget_source_compile(d, props, assets, &arena, NULL, &e));
    assert(!osg_widget_source_compile(NULL, props, assets, &arena, &result, NULL));
    assert(!memcmp(snapshot, memory, used)); free(snapshot);
}
int main(int argc, char **argv) {
    assert(argc == 2); cJSON *fixture = load(argv[1]);
    const size_t capacity = 512 * 1024; unsigned char *memory = malloc(capacity); assert(memory);
    adversarial(fixture, memory, capacity); boundaries(fixture, memory, capacity);
    osg_widget_arena arena = {.data = memory, .capacity = capacity, .max_layers = 256, .max_components = 32};
    const osg_widget_component *first = NULL, *second = NULL;
    unsigned index = 0;
    for (const cJSON *source = get(fixture, "cases")->child; source; source = source->next, index++) {
        const osg_widget_component *v; osg_widget_diagnostic diagnostic;
        assert(osg_widget_source_compile(get(source, "definition"), get(source, "properties"), get(fixture, "assets"), &arena, &v, &diagnostic)); compare(v, source);
        if (index == 0) first = v;
        if (index == 10) second = v;
    }
    assert(first && second && first != second && first->layers != second->layers);
    printf("Owned source examples: %u; layers: %zu; arena bytes: %zu\n", index, arena.layers, arena.used);
    cJSON_Delete(fixture); equal(first->layers[1].paints[OSG_WIDGET_NORMAL].text, "ENGINE"); equal(second->layers[1].paints[OSG_WIDGET_NORMAL].text, "SECOND");
    assert(first->layers[0].paints[OSG_WIDGET_NORMAL].stroke != second->layers[0].paints[OSG_WIDGET_NORMAL].stroke);
    free(memory); puts("Portable source admission, state resolution and immutable ownership passed"); return 0;
}
