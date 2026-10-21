#include "lvgl_backend.h"
#include <math.h>
#include <string.h>

static bool initialized;
static void flush(lv_display_t *display, const lv_area_t *area, uint8_t *pixels) {
    osg_lvgl_backend *backend = lv_display_get_user_data(display);
    const int width = lv_area_get_width(area), height = lv_area_get_height(area);
    if (!backend->rendering || area->x1 < 0 || area->y1 < 0 || area->x2 >= backend->board.width || area->y2 >= backend->board.height || width <= 0 || height <= 0 ||
        !backend->board.write(backend->board.context, area->x1, area->y1, width, height, (const uint16_t *)pixels)) backend->failed = true;
    backend->flushes++;
    lv_display_flush_ready(display);
}
static void draw(lv_event_t *event) {
    osg_lvgl_backend *backend = lv_event_get_user_data(event);
    if (backend->rendering && !backend->paint(backend->paint_context, lv_event_get_layer(event))) backend->failed = true;
}
bool osg_lvgl_init(osg_lvgl_backend *backend, osg_board board, void *buffer, size_t bytes) {
    if (!backend || !board.write || board.width < 32 || board.width > 1024 || board.height < 32 || board.height > 1024 || !buffer || (uintptr_t)buffer % LV_DRAW_BUF_ALIGN || bytes < (size_t)board.width * 2 || bytes > UINT32_MAX) return false;
    memset(backend, 0, sizeof(*backend)); backend->board = board;
    if (!initialized) { lv_init(); initialized = true; }
    backend->display = lv_display_create(board.width, board.height);
    if (!backend->display) return false;
    lv_display_set_color_format(backend->display, LV_COLOR_FORMAT_RGB565);
    lv_display_set_buffers(backend->display, buffer, NULL, (uint32_t)bytes, LV_DISPLAY_RENDER_MODE_PARTIAL);
    lv_display_set_user_data(backend->display, backend);
    lv_display_set_flush_cb(backend->display, flush);
    backend->surface = lv_display_get_screen_active(backend->display);
    lv_obj_remove_style_all(backend->surface);
    lv_obj_remove_flag(backend->surface, LV_OBJ_FLAG_SCROLLABLE);
    lv_obj_set_style_bg_color(backend->surface, lv_color_hex(0x0d1720), 0);
    lv_obj_set_style_bg_opa(backend->surface, LV_OPA_COVER, 0);
    lv_obj_add_event_cb(backend->surface, draw, LV_EVENT_DRAW_MAIN, backend);
    return true;
}
void osg_lvgl_destroy(osg_lvgl_backend *backend) {
    if (!backend || !backend->display || backend->rendering) return;
    lv_display_delete(backend->display); memset(backend, 0, sizeof(*backend));
}
bool osg_lvgl_render(osg_lvgl_backend *backend, osg_lvgl_paint paint, void *context) {
    if (!backend || !backend->display || !paint || backend->rendering) return false;
    backend->failed = false; backend->paint = paint; backend->paint_context = context; backend->rendering = true;
    const uint32_t before = backend->flushes;
    lv_obj_invalidate(backend->surface); lv_refr_now(backend->display);
    if (backend->flushes == before) backend->failed = true;
    if (!backend->failed && backend->board.present && !backend->board.present(backend->board.context)) backend->failed = true;
    backend->rendering = false; backend->paint = NULL; backend->paint_context = NULL;
    return !backend->failed;
}
static bool clip(lv_layer_t *layer, const lv_area_t *area, lv_area_t *previous) {
    *previous = layer->_clip_area;
    layer->_clip_area = (lv_area_t){LV_MAX(previous->x1, area->x1), LV_MAX(previous->y1, area->y1), LV_MIN(previous->x2, area->x2), LV_MIN(previous->y2, area->y2)};
    return layer->_clip_area.x1 <= layer->_clip_area.x2 && layer->_clip_area.y1 <= layer->_clip_area.y2;
}
void osg_lvgl_rect(lv_layer_t *layer, lv_area_t area, uint32_t fill, uint32_t stroke, unsigned stroke_width, unsigned radius, uint8_t opacity) {
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    lv_draw_rect_dsc_t d; lv_draw_rect_dsc_init(&d);
    d.bg_color = lv_color_hex(fill); d.bg_opa = fill == OSG_LVGL_TRANSPARENT ? 0 : opacity; d.border_color = lv_color_hex(stroke); d.border_width = (int32_t)stroke_width; d.border_opa = stroke == OSG_LVGL_TRANSPARENT ? 0 : opacity; d.radius = (int32_t)radius;
    lv_draw_rect(layer, &d, &area); layer->_clip_area = previous;
}
void osg_lvgl_line(lv_layer_t *layer, lv_area_t area, uint32_t stroke, unsigned width, uint8_t opacity) {
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    lv_draw_line_dsc_t d; lv_draw_line_dsc_init(&d);
    d.p1 = (lv_point_precise_t){area.x1, (area.y1 + area.y2) / 2.0f}; d.p2 = (lv_point_precise_t){area.x2, d.p1.y};
    d.color = lv_color_hex(stroke); d.width = (int32_t)width; d.opa = opacity;
    lv_draw_line(layer, &d); layer->_clip_area = previous;
}
void osg_lvgl_ellipse(lv_layer_t *layer, lv_area_t area, uint32_t fill, uint32_t stroke, unsigned stroke_width, uint8_t opacity) {
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    const double rx = lv_area_get_width(&area) / 2.0, ry = lv_area_get_height(&area) / 2.0, cx = area.x1 + rx, cy = area.y1 + ry;
    const double ix = fmax(0, rx - stroke_width), iy = fmax(0, ry - stroke_width);
    lv_draw_rect_dsc_t d; lv_draw_rect_dsc_init(&d); d.bg_opa = opacity; d.border_width = 0; d.radius = 0;
    // Elliptical scan spans avoid alpha seams from overlapping triangles.
    // Only rows in this LVGL partial buffer are queued, bounding task memory.
    for (int32_t y = layer->_clip_area.y1; y <= layer->_clip_area.y2; y++) {
        const double dy = y + .5 - cy, extent = rx * sqrt(fmax(0, 1 - dy * dy / (ry * ry)));
        const int32_t left = (int32_t)ceil(cx - extent - .5), right = (int32_t)floor(cx + extent - .5);
        const double inner = iy > 0 && fabs(dy) < iy ? ix * sqrt(fmax(0, 1 - dy * dy / (iy * iy))) : 0;
        const int32_t il = stroke_width ? (int32_t)ceil(cx - inner - .5) : left, ir = stroke_width ? (int32_t)floor(cx + inner - .5) : right;
        if (fill != OSG_LVGL_TRANSPARENT && il <= ir) { lv_area_t span = {il, y, ir, y}; d.bg_color = lv_color_hex(fill); lv_draw_rect(layer, &d, &span); }
        if (stroke_width && stroke != OSG_LVGL_TRANSPARENT) {
            d.bg_color = lv_color_hex(stroke);
            if (left < il) { lv_area_t span = {left, y, il - 1, y}; lv_draw_rect(layer, &d, &span); }
            if (ir < right) { lv_area_t span = {ir + 1, y, right, y}; lv_draw_rect(layer, &d, &span); }
        }
    }
    layer->_clip_area = previous;
}
void osg_lvgl_text(lv_layer_t *layer, lv_area_t area, const char *text, const lv_font_t *font, lv_text_align_t align, uint32_t color, uint8_t opacity) {
    if (!text || !font) return;
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    lv_draw_label_dsc_t d; lv_draw_label_dsc_init(&d); d.text = text; d.font = font; d.align = align; d.color = lv_color_hex(color); d.opa = opacity;
    d.ofs_y = (lv_area_get_height(&area) - font->line_height) / 2; d.flag = LV_TEXT_FLAG_EXPAND;
    lv_draw_label(layer, &d, &area); layer->_clip_area = previous;
}
void osg_lvgl_arc(lv_layer_t *layer, lv_area_t area, double fraction, uint32_t color, unsigned width, uint8_t opacity) {
    if (!isfinite(fraction) || fraction <= 0 || !width) return;
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    lv_draw_arc_dsc_t d; lv_draw_arc_dsc_init(&d); d.center = (lv_point_t){(area.x1 + area.x2) / 2, (area.y1 + area.y2) / 2};
    const int diameter = LV_MIN(lv_area_get_width(&area), lv_area_get_height(&area));
    d.radius = (uint16_t)(diameter * .42 + width / 2.0); d.start_angle = 135; d.end_angle = (float)(135 + 270 * fmin(1, fraction));
    d.width = (int32_t)width; d.color = lv_color_hex(color); d.opa = opacity; d.rounded = 1;
    lv_draw_arc(layer, &d); layer->_clip_area = previous;
}
void osg_lvgl_image(lv_layer_t *layer, lv_area_t area, const lv_image_dsc_t *image, uint8_t opacity) {
    if (!image || !image->header.w || !image->header.h) return;
    lv_area_t previous; if (!clip(layer, &area, &previous)) { layer->_clip_area = previous; return; }
    lv_draw_image_dsc_t d; lv_draw_image_dsc_init(&d); d.src = image; d.opa = opacity;
    d.scale_x = (uint32_t)lv_area_get_width(&area) * LV_SCALE_NONE / image->header.w;
    d.scale_y = (uint32_t)lv_area_get_height(&area) * LV_SCALE_NONE / image->header.h;
    // LVGL transforms source-sized coordinates. Passing destination dimensions
    // here would apply the scale twice and misdescribe the source pixel extent.
    lv_area_t source = {area.x1, area.y1, area.x1 + image->header.w - 1, area.y1 + image->header.h - 1};
    d.pivot = (lv_point_t){0, 0}; lv_draw_image(layer, &d, &source); layer->_clip_area = previous;
}
