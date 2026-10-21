#include "lvgl_backend.h"
#include "cJSON.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t pixels[800 * 480];
static uint16_t buffer[800 * 8] __attribute__((aligned(64)));
static unsigned writes, fences;
static bool fail_write, fail_fence;
static const cJSON *field(const cJSON *node, const char *key) { return cJSON_GetObjectItemCaseSensitive(node, key); }
static const char *string(const cJSON *node, const char *key) { return cJSON_GetStringValue(field(node, key)); }
static double number(const cJSON *node, const char *key) { const cJSON *v = field(node, key); assert(cJSON_IsNumber(v)); return v->valuedouble; }
static uint32_t color(const cJSON *node, const char *key) { const char *v = string(node, key); if (!v) return OSG_LVGL_TRANSPARENT; assert(v[0] == '#' && strlen(v) == 7); return (uint32_t)strtoul(v + 1, NULL, 16); }
static bool write_pixels(void *context, int x, int y, int w, int h, const uint16_t *data) {
    (void)context; assert(x >= 0 && y >= 0 && w > 0 && h > 0 && x + w <= 800 && y + h <= 480 && w * h <= 800 * 8);
    for (int row = 0; row < h; row++) memcpy(pixels + (y + row) * 800 + x, data + row * w, (size_t)w * 2);
    writes++; return !fail_write;
}
static bool present(void *context) { (void)context; fences++; return !fail_fence; }
static bool paint(void *context, lv_layer_t *layer) {
    const cJSON *frame = context, *nodes = field(frame, "nodes"); assert(cJSON_IsArray(nodes));
    cJSON *node;
    cJSON_ArrayForEach(node, nodes) {
        const char *kind = string(node, "kind"); const cJSON *box = field(node, "area"); assert(kind && cJSON_GetArraySize(box) == 4);
        int values[4]; for (unsigned i = 0; i < 4; i++) values[i] = cJSON_GetArrayItem(box, (int)i)->valueint;
        lv_area_t area = {values[0], values[1], values[0] + values[2] - 1, values[1] + values[3] - 1};
        const uint32_t fill = color(node, "fill"), stroke = color(node, "stroke"); const unsigned width = (unsigned)number(node, "strokeWidth"); const uint8_t opacity = (uint8_t)lround(number(node, "opacity") * 255);
        if (!strcmp(kind, "rectangle")) osg_lvgl_rect(layer, area, fill, stroke, width, (unsigned)number(node, "radius"), opacity);
        else if (!strcmp(kind, "line")) osg_lvgl_line(layer, area, stroke, width, opacity);
        else if (!strcmp(kind, "ellipse")) osg_lvgl_ellipse(layer, area, fill, stroke, width, opacity);
        else if (!strcmp(kind, "indicator")) osg_lvgl_ellipse(layer, area, cJSON_IsTrue(field(node, "active")) ? fill : 0x42515a, OSG_LVGL_TRANSPARENT, 0, opacity);
        else if (!strcmp(kind, "text")) {
            const char *align = string(node, "align");
            /* This smoke test deliberately uses one built-in font. Portable font
             * assets/metrics are a separate integration requirement, not proven here. */
            osg_lvgl_text(layer, area, string(node, "text"), LV_FONT_DEFAULT, !strcmp(align, "center") ? LV_TEXT_ALIGN_CENTER : !strcmp(align, "right") ? LV_TEXT_ALIGN_RIGHT : LV_TEXT_ALIGN_LEFT, fill, opacity);
        } else if (!strcmp(kind, "gauge")) {
            double fraction = number(node, "fraction");
            const char *orientation = string(node, "orientation");
            if (!strcmp(orientation, "radial")) { osg_lvgl_arc(layer, area, 1, stroke, width, opacity); osg_lvgl_arc(layer, area, fraction, fill, width, opacity); }
            else {
                osg_lvgl_rect(layer, area, stroke, OSG_LVGL_TRANSPARENT, 0, 0, opacity);
                if (!strcmp(orientation, "vertical")) area.y1 = area.y2 - (int32_t)lround(lv_area_get_height(&area) * fraction) + 1;
                else area.x2 = area.x1 + (int32_t)lround(lv_area_get_width(&area) * fraction) - 1;
                if (fraction > 0) osg_lvgl_rect(layer, area, fill, OSG_LVGL_TRANSPARENT, 0, 0, opacity);
            }
        } else assert(!"Unexpected shared fixture layer kind");
    }
    return true;
}
static uint64_t digest(void) { uint64_t hash = 1469598103934665603ULL; for (size_t i = 0; i < sizeof(pixels) / sizeof(*pixels); i++) hash = (hash ^ pixels[i]) * 1099511628211ULL; return hash; }
static bool near565(uint16_t value, uint16_t expected) { return abs((value >> 11) - (expected >> 11)) <= 1 && abs(((value >> 5) & 63) - ((expected >> 5) & 63)) <= 1 && abs((value & 31) - (expected & 31)) <= 1; }
static bool bad_paint(void *context, lv_layer_t *layer) { (void)context; (void)layer; return false; }
static bool primitives(void *context, lv_layer_t *layer) {
    (void)context;
    static const uint32_t rgba[] = {0xffff0000, 0xff00ff00, 0xff0000ff, 0xffffffff};
    static const lv_image_dsc_t image = {.header = {.magic = LV_IMAGE_HEADER_MAGIC, .cf = LV_COLOR_FORMAT_ARGB8888, .w = 2, .h = 2, .stride = 8}, .data_size = sizeof(rgba), .data = (const uint8_t *)rgba};
    osg_lvgl_ellipse(layer, (lv_area_t){-10, -10, 30, 20}, 0x334455, 0x112233, 2, 128);
    osg_lvgl_ellipse(layer, (lv_area_t){50, 30, 90, 50}, OSG_LVGL_TRANSPARENT, 0xffffff, 2, 255);
    osg_lvgl_rect(layer, (lv_area_t){900, 900, 950, 950}, 0xffffff, OSG_LVGL_TRANSPARENT, 0, 0, 255);
    osg_lvgl_line(layer, (lv_area_t){900, 900, 950, 950}, 0xffffff, 2, 255);
    osg_lvgl_text(layer, (lv_area_t){900, 900, 950, 950}, "Clipped", LV_FONT_DEFAULT, LV_TEXT_ALIGN_LEFT, 0xffffff, 255);
    osg_lvgl_text(layer, (lv_area_t){0, 0, 10, 10}, NULL, NULL, LV_TEXT_ALIGN_LEFT, 0xffffff, 255);
    osg_lvgl_arc(layer, (lv_area_t){900, 900, 950, 950}, .5, 0xffffff, 3, 255);
    osg_lvgl_arc(layer, (lv_area_t){0, 0, 10, 10}, NAN, 0xffffff, 0, 255);
    osg_lvgl_image(layer, (lv_area_t){900, 900, 950, 950}, &image, 255);
    osg_lvgl_image(layer, (lv_area_t){0, 0, 10, 10}, NULL, 255);
    osg_lvgl_image(layer, (lv_area_t){100, 100, 103, 103}, &image, 255);
    return true;
}
int main(int argc, char **argv) {
    assert(argc == 3); FILE *file = fopen(argv[1], "rb"); assert(file); assert(!fseek(file, 0, SEEK_END)); long bytes = ftell(file); assert(bytes > 0 && bytes <= 512000); rewind(file);
    char *data = malloc((size_t)bytes + 1); assert(data && fread(data, 1, (size_t)bytes, file) == (size_t)bytes); fclose(file); data[bytes] = 0;
    cJSON *fixture = cJSON_Parse(data); free(data); assert(fixture); const cJSON *frames = field(fixture, "frames"); assert(cJSON_GetArraySize(frames) == 8);
    osg_board board = {.width = 800, .height = 480, .write = write_pixels, .present = present}; osg_lvgl_backend backend;
    assert(!osg_lvgl_init(NULL, board, buffer, sizeof(buffer))); assert(!osg_lvgl_init(&backend, board, NULL, sizeof(buffer))); assert(!osg_lvgl_init(&backend, board, buffer, 1));
    assert(osg_lvgl_init(&backend, board, buffer, sizeof(buffer))); assert(!osg_lvgl_render(&backend, NULL, NULL));
    uint64_t hashes[8];
    for (unsigned i = 0; i < 8; i++) {
        cJSON *frame = cJSON_GetArrayItem(frames, (int)i); assert(osg_lvgl_render(&backend, paint, frame)); hashes[i] = digest(); assert(writes >= 60 && fences == i + 1);
        char path[1024]; assert(snprintf(path, sizeof(path), "%s/%s.ppm", argv[2], string(frame, "state")) > 0); file = fopen(path, "wb"); assert(file); fprintf(file, "P6\n800 480\n255\n");
        for (size_t j = 0; j < sizeof(pixels) / sizeof(*pixels); j++) { unsigned char rgb[] = {(unsigned char)((pixels[j] >> 11) * 255 / 31), (unsigned char)(((pixels[j] >> 5) & 63) * 255 / 63), (unsigned char)((pixels[j] & 31) * 255 / 31)}; assert(fwrite(rgb, 1, 3, file) == 3); } fclose(file);
    }
    assert(hashes[0] != hashes[1] && hashes[0] != hashes[2] && hashes[2] != hashes[5] && hashes[5] != hashes[6]);
    unsigned before = fences; fail_write = true; assert(!osg_lvgl_render(&backend, paint, cJSON_GetArrayItem(frames, 0))); assert(fences == before);
    fail_write = false; fail_fence = true; assert(!osg_lvgl_render(&backend, paint, cJSON_GetArrayItem(frames, 0))); fail_fence = false;
    assert(!osg_lvgl_render(&backend, bad_paint, NULL)); assert(osg_lvgl_render(&backend, paint, cJSON_GetArrayItem(frames, 0)));
    assert(osg_lvgl_render(&backend, primitives, NULL));
    // The software image filter rounds alpha/color to RGB565, with at most one
    // channel step of quantization at each corner of this scaled 2x2 fixture.
    assert(near565(pixels[100 * 800 + 100], 0xf800)); assert(near565(pixels[100 * 800 + 103], 0x07e0));
    assert(near565(pixels[103 * 800 + 100], 0x001f)); assert(near565(pixels[103 * 800 + 103], 0xffff));
    osg_lvgl_destroy(NULL); osg_lvgl_destroy(&backend); osg_lvgl_destroy(&backend); assert(!osg_lvgl_render(&backend, paint, NULL)); cJSON_Delete(fixture);
    puts("Locked LVGL shared-source geometry/state smoke test passed; fonts, live delivery and hardware qualification remain separate.");
    return 0;
}
