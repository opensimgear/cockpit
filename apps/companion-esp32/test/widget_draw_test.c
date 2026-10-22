#include "widget_draw.h"
#include "widget_source.h"
#include "cJSON.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t pixels[800 * 480], buffer[800 * 8] __attribute__((aligned(64)));
static osg_widget_fonts fonts;
static const osg_widget_component *components[16];
static lv_area_t positions[16];
static unsigned component_count, writes;
static bool fail_write;
static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static const char *str(const cJSON *v, const char *key) { return cJSON_GetStringValue(get(v, key)); }
static double num(const cJSON *v, const char *key) { const cJSON *field = get(v, key); assert(cJSON_IsNumber(field)); return field->valuedouble; }
static cJSON *load(const char *path) {
    FILE *f = fopen(path, "rb"); assert(f); assert(!fseek(f, 0, SEEK_END)); const long size = ftell(f); assert(size > 0 && size < 4 * 1024 * 1024); rewind(f);
    char *bytes = malloc((size_t)size + 1); assert(bytes && fread(bytes, 1, (size_t)size, f) == (size_t)size); bytes[size] = 0; fclose(f);
    cJSON *result = cJSON_Parse(bytes); free(bytes); assert(result); return result;
}
static bool write_pixels(void *context, int x, int y, int width, int height, const uint16_t *data) {
    (void)context; assert(x >= 0 && y >= 0 && width > 0 && height > 0 && x + width <= 800 && y + height <= 480 && width * height <= 800 * 8);
    for (int row = 0; row < height; row++) memcpy(pixels + (y + row) * 800 + x, data + row * width, (size_t)width * 2);
    writes++; return !fail_write;
}
static osg_sample sample(const cJSON *v) {
    osg_sample s = {.received_ms = 100, .source_age_ms = (uint32_t)num(v, "sourceAgeMs"), .provenance = !strcmp(str(v, "provenance"), "requested") ? OSG_REQUESTED : OSG_GAME_CONFIRMED};
    const char *status = str(v, "status");
    if (!strcmp(status, "unknown")) s.source_status = OSG_FEEDBACK_UNKNOWN;
    else if (!strcmp(status, "stale")) s.source_status = OSG_FEEDBACK_STALE;
    else if (!strcmp(status, "disconnected")) s.source_status = OSG_FEEDBACK_DISCONNECTED;
    const cJSON *value = get(v, "value"); s.available = !cJSON_IsNull(value);
    if (cJSON_IsBool(value)) { s.kind = OSG_BOOL; s.boolean = cJSON_IsTrue(value); }
    else if (cJSON_IsNumber(value)) { s.kind = OSG_NUMBER; s.number = value->valuedouble; }
    else if (cJSON_IsString(value)) { s.kind = OSG_STRING; assert(strlen(value->valuestring) < sizeof(s.text)); strcpy(s.text, value->valuestring); }
    return s;
}
static bool paint(void *value, lv_layer_t *layer) {
    const cJSON *frame = value; const cJSON *contexts = get(frame, "contexts");
    for (unsigned i = 0; i < component_count; i++) {
        const osg_widget_component *component = components[i]; const cJSON *source = cJSON_GetArrayItem(contexts, (int)i);
        osg_widget_state_context context = {.now_ms = 100, .connected = cJSON_IsTrue(get(source, "connected")), .enabled = cJSON_IsTrue(get(source, "enabled")), .pressed = cJSON_IsTrue(get(source, "pressed"))};
        osg_sample storage[8] = {0};
        for (unsigned port = 0; port < component->output_count; port++) {
            const cJSON *s = get(get(source, "samples"), component->outputs[port].name); assert(s); storage[port] = sample(s);
            context.outputs[port] = (osg_widget_sample_view){&storage[port], true};
        }
        const char *states[] = {"normal", "active", "pressed", "warning", "stale", "unknown", "disabled", "disconnected"}; const osg_widget_state state = osg_widget_state_resolve(component, &context);
        assert(!strcmp(states[state], str(source, "state")));
        for (unsigned l = 0; l < component->layer_count; l++) if (component->layers[l].kind == OSG_LAYER_TEXT) {
            char text[512]; assert(osg_widget_layer_text(&component->layers[l], &context, state, text, sizeof(text)));
            assert(!strcmp(text, cJSON_GetStringValue(cJSON_GetArrayItem(get(source, "texts"), (int)l))));
        }
        osg_widget_draw_diagnostic diagnostic;
        if (!osg_widget_draw(layer, component, positions[i], &context, &fonts, NULL, NULL, &diagnostic)) { fprintf(stderr, "Draw %s/%s failed: %d %zu/%zu\n", component->id, diagnostic.layer, diagnostic.code, diagnostic.required, diagnostic.available); return false; }
    }
    return true;
}
static void text_cases(const cJSON *fixture) {
    osg_widget_layer layer = components[4]->layers[2]; assert(layer.kind == OSG_LAYER_TEXT); unsigned count = 0;
    for (const cJSON *c = get(fixture, "textCases")->child; c; c = c->next, count++) {
        cJSON *v = cJSON_Duplicate(c, true); assert(v);
        assert(cJSON_AddNumberToObject(v, "sourceAgeMs", num(c, "age")) && cJSON_AddStringToObject(v, "provenance", "game-confirmed"));
        osg_sample s = sample(v); osg_widget_state_context context = {.now_ms = 100}; context.outputs[layer.output] = (osg_widget_sample_view){&s, true};
        layer.precision = (uint8_t)num(c, "precision"); layer.unit = str(c, "unit"); char text[512];
        assert(osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, text, sizeof(text)));
        if (strcmp(text, str(c, "text"))) { fprintf(stderr, "Text case %u: %s != %s\n", count, text, str(c, "text")); abort(); }
        assert(osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, text, 1) == !*str(c, "text")); cJSON_Delete(v);
    }
    printf("Exact shared live text projections: %u\n", count);
}
static bool bitmap_lookup(void *context, const char *id, unsigned width, unsigned height, osg_widget_bitmap *out) {
    assert(!strcmp(id, "prepared") && width == 200 && height == 120);
    static const unsigned char pixels[] = {0x00, 0xf8, 255, 0xe0, 0x07, 128, 0x1f, 0x00, 0, 0xff, 0xff, 255};
    *out = (osg_widget_bitmap){pixels, 2, 2};
    const unsigned mode = *(const unsigned *)context;
    if (mode == 1) out->pixels = NULL;
    if (mode == 2) out->width = 0;
    if (mode == 3) out->height = 1025;
    return mode != 4;
}
static bool probes(void *value, lv_layer_t *layer) {
    const unsigned mode = *(const unsigned *)value;
    osg_widget_component component = *components[0]; osg_widget_layer visual = component.layers[0];
    component.layers = &visual; component.layer_count = 1; component.confirmed_output = component.warning_output = component.enabled_output = -1;
    visual.bounds = (osg_bounds){0, 0, 1, 1}; visual.output = 0; visual.min = 0; visual.max = 1; visual.radius = 10;
    for (unsigned i = 0; i < 8; i++) visual.paints[i] = (osg_widget_paint){.fill = 0x00ff00, .stroke = 0x0000ff, .stroke_width = 4, .has_stroke_width = true, .visible = true, .opacity = 1, .text = " "};
    osg_sample number = {.available = true, .kind = OSG_NUMBER, .number = .5};
    osg_widget_state_context context = {.connected = true, .enabled = true}; context.outputs[0] = (osg_widget_sample_view){&number, true};
    const lv_area_t area = {0, 0, 199, 119}; osg_widget_draw_diagnostic e;
    if (mode == 0) { visual.kind = OSG_LAYER_GAUGE; visual.orientation = 1; }
    if (mode == 1) visual.kind = OSG_LAYER_ELLIPSE;
    if (mode == 2) { visual.kind = OSG_LAYER_LINE; visual.paints[0].stroke = visual.paints[0].fill = OSG_WIDGET_SOURCE_NO_COLOR; visual.paints[0].has_stroke_width = false; }
    if (mode == 3) { visual.kind = OSG_LAYER_IMAGE; visual.asset_id = "prepared"; visual.paints[0].opacity = .5; }
    if (mode == 4) { visual.kind = OSG_LAYER_GAUGE; visual.orientation = 2; number.number = 1; visual.paints[0].has_stroke_width = false; visual.paints[0].stroke = visual.paints[0].fill = OSG_WIDGET_SOURCE_NO_COLOR; }
    if (mode == 5) { visual.kind = OSG_LAYER_TEXT; visual.font_size = 22; visual.font = OSG_FONT_SANS; visual.output = -1; }
    unsigned bitmap_mode = 0;
    assert(osg_widget_draw(layer, &component, area, &context, &fonts, bitmap_lookup, &bitmap_mode, &e));
    if (layer->_clip_area.y1 != 0) return true;
    assert(!osg_widget_draw(NULL, &component, area, &context, &fonts, NULL, NULL, &e) && e.code == OSG_WIDGET_DRAW_ARGUMENT);
    assert(!osg_widget_draw(layer, NULL, area, &context, &fonts, NULL, NULL, &e));
    assert(!osg_widget_draw(layer, &component, area, NULL, &fonts, NULL, NULL, &e));
    assert(!osg_widget_draw(layer, &component, area, &context, NULL, NULL, NULL, &e));
    assert(!osg_widget_draw(layer, &component, (lv_area_t){0, 0, -1, 1}, &context, &fonts, NULL, NULL, &e));
    assert(!osg_widget_draw(layer, &component, (lv_area_t){0, 0, 9999, 9999}, &context, &fonts, NULL, NULL, &e) && e.code == OSG_WIDGET_DRAW_BUDGET);
    lv_layer_t invalid = *layer; invalid._clip_area = (lv_area_t){0, 0, 199, 8};
    assert(!osg_widget_draw(&invalid, &component, area, &context, &fonts, NULL, NULL, &e) && e.required == 9 && e.available == 8);
    invalid._clip_area = (lv_area_t){0, 0, 1024, 7};
    assert(!osg_widget_draw(&invalid, &component, (lv_area_t){0, 0, 1024, 119}, &context, &fonts, NULL, NULL, &e) && e.required == 1025 && e.available == 1024);
    visual.kind = OSG_LAYER_IMAGE; visual.asset_id = "prepared";
    assert(!osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, &e) && e.code == OSG_WIDGET_DRAW_ASSET);
    for (bitmap_mode = 1; bitmap_mode <= 4; bitmap_mode++) assert(!osg_widget_draw(layer, &component, area, &context, &fonts, bitmap_lookup, &bitmap_mode, &e) && e.code == OSG_WIDGET_DRAW_ASSET);
    visual.asset_id = ""; assert(osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, &e));
    visual.kind = OSG_LAYER_TEXT; visual.output = -1; visual.font = OSG_FONT_SANS; visual.font_size = 22; visual.align = 0; visual.paints[0].text = "\xf0\x9f\x98\x83";
    assert(!osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, &e) && e.code == OSG_WIDGET_DRAW_TEXT);
    visual.kind = OSG_LAYER_RECTANGLE; visual.paints[0].opacity = 0;
    assert(osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, NULL));
    visual.paints[0].opacity = .5; visual.paints[0].visible = false;
    assert(osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, NULL));
    visual.paints[0].visible = true;
    lv_mem_monitor_t available; lv_mem_monitor(&available); assert(available.free_biggest_size > 4096);
    void *reserve = lv_malloc(available.free_biggest_size - 2048); assert(reserve);
    assert(!osg_widget_draw(layer, &component, area, &context, &fonts, NULL, NULL, &e) && e.code == OSG_WIDGET_DRAW_BUDGET && e.required > e.available);
    lv_free(reserve); return true;
}
static void text_limits(void) {
    osg_widget_layer layer = components[4]->layers[2]; char out[512]; osg_sample sample = {.available = true, .kind = OSG_NUMBER};
    osg_widget_state_context context = {0}; context.outputs[layer.output] = (osg_widget_sample_view){&sample, true};
    assert(!osg_widget_layer_text(NULL, &context, OSG_WIDGET_NORMAL, out, sizeof(out)));
    assert(!osg_widget_layer_text(&layer, NULL, OSG_WIDGET_NORMAL, out, sizeof(out)));
    assert(!osg_widget_layer_text(&layer, &context, (osg_widget_state)8, out, sizeof(out)));
    assert(!osg_widget_layer_text(&layer, &context, (osg_widget_state)-1, out, sizeof(out)));
    assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, NULL, sizeof(out)));
    for (unsigned i = 0; i < 3; i++) { sample.number = i == 0 ? NAN : i == 1 ? INFINITY : 1e21; assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, sizeof(out))); }
    sample.number = 12.34; layer.precision = 5; assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, sizeof(out)));
    layer.precision = 2; layer.unit = "%"; assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, 6));
    sample.kind = OSG_STRING; memset(sample.text, 'A', sizeof(sample.text)); assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, sizeof(out)));
    sample.kind = (osg_value_kind)8; assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, sizeof(out)));
    layer.kind = OSG_LAYER_LINE; assert(!osg_widget_layer_text(&layer, &context, OSG_WIDGET_NORMAL, out, sizeof(out)));
}
static void prepared_bitmap_checks(void) {
    const unsigned char pixels[] = {0,248,128,224,7,255};
    osg_bound_bitmap bitmaps[] = {{.asset_id = "art", .width = 2, .height = 1, .pixels = pixels}, {.asset_id = "art", .width = 1, .height = 2, .pixels = pixels}};
    osg_widget_binding binding = {.bitmaps = bitmaps, .bitmap_count = 2}; osg_widget_bitmap out;
    assert(osg_widget_bound_bitmap(&binding,"art",2,1,&out) && out.pixels == pixels && out.width == 2 && out.height == 1);
    assert(osg_widget_bound_bitmap(&binding,"art",1,2,&out) && out.width == 1 && out.height == 2);
    assert(!osg_widget_bound_bitmap(&binding,"other",2,1,&out) && !out.pixels);
    assert(!osg_widget_bound_bitmap(&binding,"art",1,1,&out)); bitmaps[0].pixels = NULL; assert(!osg_widget_bound_bitmap(&binding,"art",2,1,&out));
    bitmaps[0].asset_id = NULL; assert(!osg_widget_bound_bitmap(&binding,"art",2,1,&out));
    assert(!osg_widget_bound_bitmap(NULL,"art",2,1,&out)); assert(!osg_widget_bound_bitmap(&binding,NULL,2,1,&out));
    assert(!osg_widget_bound_bitmap(&binding,"art",0,1,&out)); assert(!osg_widget_bound_bitmap(&binding,"art",2,0,&out));
    assert(!osg_widget_bound_bitmap(&binding,"art",2,1,NULL)); binding.bitmap_count = 33; assert(!osg_widget_bound_bitmap(&binding,"art",2,1,&out)); binding.bitmap_count = 2;
    binding.bitmaps = NULL; assert(!osg_widget_bound_bitmap(&binding,"art",2,1,&out));
}
static void ppm(const char *directory, const char *name) {
    char path[1024]; assert(snprintf(path, sizeof(path), "%s/source-%s.ppm", directory, name) < (int)sizeof(path)); FILE *f = fopen(path, "wb"); assert(f); fprintf(f, "P6\n800 480\n255\n");
    for (unsigned i = 0; i < 800 * 480; i++) { const uint16_t p = pixels[i]; const unsigned char rgb[] = {(unsigned char)((p >> 11) * 255 / 31), (unsigned char)(((p >> 5) & 63) * 255 / 63), (unsigned char)((p & 31) * 255 / 31)}; assert(fwrite(rgb, 1, 3, f) == 3); }
    assert(!fclose(f));
}
int main(int argc, char **argv) {
    assert(argc == 3); cJSON *fixture = load(argv[1]), *assets = cJSON_CreateArray(); assert(assets);
    unsigned char *memory = malloc(128 * 1024); assert(memory); osg_widget_arena arena = {.data = memory, .capacity = 128 * 1024, .max_layers = 128, .max_components = 16};
    for (const cJSON *w = get(fixture, "widgets")->child; w; w = w->next) {
        const cJSON *b = get(w, "bounds"); const unsigned i = component_count++; assert(i < 16);
        assert(osg_widget_source_compile(get(w, "definition"), get(w, "properties"), assets, &arena, &components[i], NULL));
        positions[i] = (lv_area_t){(int32_t)lround(num(b, "x") * 800), (int32_t)lround(num(b, "y") * 480), (int32_t)lround((num(b, "x") + num(b, "width")) * 800) - 1, (int32_t)lround((num(b, "y") + num(b, "height")) * 480) - 1};
    }
    cJSON_Delete(assets); text_cases(fixture); text_limits(); prepared_bitmap_checks();
    osg_lvgl_backend backend; assert(osg_lvgl_init(&backend, (osg_board){.width = 800, .height = 480, .write = write_pixels}, buffer, sizeof(buffer)));
    for (const cJSON *frame = get(fixture, "frames")->child; frame; frame = frame->next) {
        assert(osg_lvgl_render(&backend, paint, (void *)frame)); ppm(argv[2], str(frame, "name"));
    }
    fail_write = true; assert(!osg_lvgl_render(&backend, paint, (void *)get(fixture, "frames")->child)); fail_write = false;
    assert(osg_lvgl_render(&backend, paint, (void *)get(fixture, "frames")->child));
    for (unsigned mode = 0; mode < 6; mode++) {
        assert(osg_lvgl_render(&backend, probes, &mode));
        if (mode == 0) { assert(pixels[20 * 800 + 20] == 0x001f); assert(pixels[100 * 800 + 20] == 0x07e0); }
        if (mode == 1) assert(pixels[60 * 800 + 100] == 0x07e0);
        if (mode == 2) assert(pixels[60 * 800 + 100] == 0xffff);
        if (mode == 3) assert(pixels[10 * 800 + 10] != pixels[100 * 800 + 10]);
    }
    osg_widget_fonts_destroy(&fonts); osg_lvgl_destroy(&backend); cJSON_Delete(fixture); free(memory);
    printf("Admitted-source dynamic LVGL frames passed; partial board writes: %u\n", writes); return 0;
}
