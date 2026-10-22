#include "widget_draw.h"
#include <math.h>
#include <stdio.h>
#include <string.h>

bool osg_widget_bound_bitmap(void *context, const char *asset_id, unsigned width, unsigned height, osg_widget_bitmap *out) {
    if (!out) return false;
    *out = (osg_widget_bitmap){0};
    const osg_widget_binding *binding = context;
    if (!binding || !asset_id || !width || !height || !binding->bitmaps || binding->bitmap_count > 32) return false;
    for (unsigned i = 0; i < binding->bitmap_count; i++) {
        const osg_bound_bitmap *bitmap = &binding->bitmaps[i];
        if (bitmap->asset_id && !strcmp(bitmap->asset_id, asset_id) && bitmap->width == width && bitmap->height == height && bitmap->pixels) {
            *out = (osg_widget_bitmap){bitmap->pixels, bitmap->width, bitmap->height}; return true;
        }
    }
    return false;
}
static bool copy(char *out, size_t capacity, const char *value) {
    if (!out || !value || strlen(value) >= capacity) return false;
    memcpy(out, value, strlen(value) + 1); return true;
}
/* Exact toFixed rounding in the supported magnitude range. Multiplying a
 * binary64 significand by 5^precision fits in uint64_t (precision <= 4). A
 * 96-bit integer holds the shifted result. This avoids both printf's ties-even
 * rounding and the double rounding of floor(value * 10^precision + .5). */
static bool fixed(double value, unsigned precision, char *out, size_t capacity) {
    if (!isfinite(value) || fabs(value) >= 1e21 || precision > 4) return false;
    int exponent; const bool negative = value < 0;
    uint64_t significand = (uint64_t)ldexp(frexp(fabs(value), &exponent), 53);
    for (unsigned i = 0; i < precision; i++) significand *= 5;
    int shift = exponent - 53 + (int)precision;
    if (shift < 0) {
        const unsigned right = (unsigned)-shift;
        if (right >= 64) significand = 0;
        else { const uint64_t remainder = significand & ((UINT64_C(1) << right) - 1); significand = (significand >> right) + (remainder >= (UINT64_C(1) << (right - 1))); }
        shift = 0;
    }
    uint32_t words[3] = {(uint32_t)significand, (uint32_t)(significand >> 32), 0};
    while (shift-- > 0) { words[2] = words[2] << 1 | words[1] >> 31; words[1] = words[1] << 1 | words[0] >> 31; words[0] <<= 1; }
    char digits[32]; unsigned length = 0;
    do {
        uint64_t remainder = 0;
        for (int i = 2; i >= 0; i--) { const uint64_t part = remainder << 32 | words[i]; words[i] = (uint32_t)(part / 10); remainder = part % 10; }
        digits[length++] = (char)('0' + remainder);
    } while (words[0] || words[1] || words[2]);
    while (length <= precision) digits[length++] = '0';
    const size_t bytes = length + (precision ? 1 : 0) + (negative ? 1 : 0);
    if (!out || bytes >= capacity) return false;
    size_t used = 0; if (negative) out[used++] = '-';
    for (unsigned i = length; i > 0; i--) { out[used++] = digits[i - 1]; if (precision && i - 1 == precision) out[used++] = '.'; }
    out[used] = 0; return true;
}
bool osg_widget_layer_text(const osg_widget_layer *layer, const osg_widget_state_context *context, osg_widget_state state, char *out, size_t capacity) {
    if (!layer || layer->kind != OSG_LAYER_TEXT || !context || state < OSG_WIDGET_NORMAL || state > OSG_WIDGET_DISCONNECTED) return false;
    if (layer->output < 0) return copy(out, capacity, layer->paints[state].text);
    const osg_sample *current = osg_widget_current_sample(context, layer->output);
    if (!current) {
        const osg_sample *previous = layer->output < OSG_WIDGET_SOURCE_PORTS ? context->outputs[layer->output].value : NULL;
        return copy(out, capacity, previous && (previous->source_status == OSG_FEEDBACK_STALE || previous->source_status == OSG_FEEDBACK_AUTOMATIC) ? "STALE" : "—");
    }
    if (current->kind == OSG_STRING) return memchr(current->text, 0, sizeof(current->text)) && copy(out, capacity, current->text);
    if (current->kind == OSG_BOOL) return copy(out, capacity, current->boolean ? "true" : "false");
    if (current->kind != OSG_NUMBER || !fixed(current->number, layer->precision, out, capacity)) return false;
    if (layer->unit && *layer->unit) {
        const size_t used = strlen(out), unit = strlen(layer->unit);
        if (unit + 1 >= capacity - used) return false;
        out[used] = ' '; memcpy(out + used + 1, layer->unit, unit + 1);
    }
    return true;
}
static bool ellipse(double x, double y, double width, double height) {
    if (width <= 0 || height <= 0) return false;
    const double dx = 2 * x / width - 1, dy = 2 * y / height - 1;
    return dx * dx + dy * dy <= 1;
}
static bool rectangle(double x, double y, double width, double height, double radius) {
    if (width <= 0 || height <= 0 || x < 0 || y < 0 || x >= width || y >= height) return false;
    radius = fmin(radius, fmin(width, height) / 2);
    const double dx = fmax(0, fmax(radius - x, x - width + radius)), dy = fmax(0, fmax(radius - y, y - height + radius));
    return dx * dx + dy * dy <= radius * radius;
}
static bool arc(double x, double y, double width, double height, double stroke, double fraction) {
    if (stroke <= 0 || fraction <= 0) return false;
    const double pi = 3.14159265358979323846, radius = fmin(width, height) * .42, half = stroke / 2;
    const double dx = x - width / 2, dy = y - height / 2, distance = dx * dx + dy * dy;
    double angle = atan2(dy, dx) - pi * .75; if (angle < 0) angle += 2 * pi;
    if (distance >= fmax(0, radius - half) * fmax(0, radius - half) && distance <= (radius + half) * (radius + half) && angle <= fraction * pi * 1.5) return true;
    const double start_x = radius * cos(pi * .75), start_y = radius * sin(pi * .75), end_x = radius * cos(pi * (.75 + 1.5 * fraction)), end_y = radius * sin(pi * (.75 + 1.5 * fraction));
    return (dx - start_x) * (dx - start_x) + (dy - start_y) * (dy - start_y) <= half * half || (dx - end_x) * (dx - end_x) + (dy - end_y) * (dy - end_y) <= half * half;
}
typedef struct {
    const osg_widget_layer *layer;
    const osg_widget_paint *paint;
    double width, height, fraction;
    bool active;
} shape;
static uint32_t shape_pixel(const shape *s, double x, double y) {
    const osg_widget_layer *layer = s->layer; const osg_widget_paint *p = s->paint;
    const uint32_t transparent = OSG_WIDGET_SOURCE_NO_COLOR, fill = p->fill, stroke = p->stroke;
    if (layer->kind == OSG_LAYER_LINE) return fabs(y - s->height / 2) <= (p->has_stroke_width ? p->stroke_width : 1) / 2 ? stroke != transparent ? stroke : fill != transparent ? fill : 0xffffff : transparent;
    if (layer->kind == OSG_LAYER_INDICATOR) return ellipse(x, y, s->width, s->height) ? s->active ? fill != transparent ? fill : 0x63dfd0 : 0x42515a : transparent;
    if (layer->kind == OSG_LAYER_GAUGE) {
        const uint32_t foreground = fill == transparent ? 0x63dfd0 : fill, background = stroke == transparent ? 0x30434d : stroke;
        if (layer->orientation == 2) {
            const double width = p->has_stroke_width ? p->stroke_width : 6;
            if (arc(x, y, s->width, s->height, width, s->fraction)) return foreground;
            return arc(x, y, s->width, s->height, width, 1) ? background : transparent;
        }
        return (layer->orientation == 1 ? y >= s->height * (1 - s->fraction) : x < s->width * s->fraction) ? foreground : background;
    }
    const double half = p->stroke_width / 2;
    const bool oval = layer->kind == OSG_LAYER_ELLIPSE;
    if (stroke != transparent && half > 0) {
        const bool outer = oval ? ellipse(x + half, y + half, s->width + 2 * half, s->height + 2 * half) : rectangle(x + half, y + half, s->width + 2 * half, s->height + 2 * half, layer->radius + half);
        const bool inner = oval ? ellipse(x - half, y - half, s->width - 2 * half, s->height - 2 * half) : rectangle(x - half, y - half, s->width - 2 * half, s->height - 2 * half, fmax(0, layer->radius - half));
        if (outer && !inner) return stroke;
    }
    return (oval ? ellipse(x, y, s->width, s->height) : rectangle(x, y, s->width, s->height, layer->radius)) ? fill : transparent;
}
static void span(lv_layer_t *layer, int x1, int x2, int y, uint32_t color, uint8_t opacity) {
    if (x2 < x1 || color == OSG_WIDGET_SOURCE_NO_COLOR || !opacity) return;
    lv_draw_rect_dsc_t d; lv_draw_rect_dsc_init(&d); d.bg_color = lv_color_hex(color); d.bg_opa = opacity; d.border_width = 0; d.radius = 0;
    const lv_area_t area = {x1, y, x2, y}; lv_draw_rect(layer, &d, &area);
    while (layer->draw_task_head) lv_draw_dispatch();
}
static bool error(osg_widget_draw_diagnostic *out, osg_widget_draw_error code, const char *layer, size_t required, size_t available) {
    *out = (osg_widget_draw_diagnostic){code, layer, required, available}; return false;
}
bool osg_widget_draw(lv_layer_t *target, const osg_widget_component *component, lv_area_t area, const osg_widget_state_context *context, osg_widget_fonts *fonts, osg_widget_bitmap_lookup lookup, void *assets, osg_widget_draw_diagnostic *diagnostic) {
    osg_widget_draw_diagnostic local; if (!diagnostic) diagnostic = &local; memset(diagnostic, 0, sizeof(*diagnostic));
    if (!target || !component || !context || !fonts || !component->width || !component->height || area.x2 < area.x1 || area.y2 < area.y1) return error(diagnostic, OSG_WIDGET_DRAW_ARGUMENT, NULL, 0, 0);
    const double sx = (double)lv_area_get_width(&area) / component->width, sy = (double)lv_area_get_height(&area) / component->height;
    if (sx > 32 || sy > 32) return error(diagnostic, OSG_WIDGET_DRAW_BUDGET, NULL, 0, 0);
    const osg_widget_state state = osg_widget_state_resolve(component, context);
    for (unsigned index = 0; index < component->layer_count; index++) {
        const osg_widget_layer *layer = &component->layers[index]; const osg_widget_paint *paint = &layer->paints[state];
        if (!paint->visible || paint->opacity <= 0) continue;
        const double left = area.x1 + layer->bounds.x * lv_area_get_width(&area), top = area.y1 + layer->bounds.y * lv_area_get_height(&area);
        const double width = layer->bounds.width * component->width, height = layer->bounds.height * component->height;
        const lv_area_t visible = {LV_MAX(target->_clip_area.x1, (int32_t)ceil(left - .5)), LV_MAX(target->_clip_area.y1, (int32_t)ceil(top - .5)), LV_MIN(target->_clip_area.x2, (int32_t)ceil(left + width * sx - .5) - 1), LV_MIN(target->_clip_area.y2, (int32_t)ceil(top + height * sy - .5) - 1)};
        if (visible.x2 < visible.x1 || visible.y2 < visible.y1) continue;
        const size_t columns = (size_t)lv_area_get_width(&visible);
        if (columns > 1024) return error(diagnostic, OSG_WIDGET_DRAW_BUDGET, layer->id, columns, 1024);
        char text[OSG_WIDGET_DRAW_TEXT_BYTES]; osg_widget_bitmap bitmap = {0};
        if (layer->kind == OSG_LAYER_TEXT) {
            osg_widget_text_metrics metrics;
            if (!osg_widget_layer_text(layer, context, state, text, sizeof(text)) || !osg_widget_text_measure(fonts, (osg_widget_font)layer->font, layer->font_size, text, &metrics)) return error(diagnostic, OSG_WIDGET_DRAW_TEXT, layer->id, 0, 0);
        }
        if (layer->kind == OSG_LAYER_IMAGE) {
            if (!*layer->asset_id) continue;
            if (!lookup || !lookup(assets, layer->asset_id, (unsigned)lround(width * sx), (unsigned)lround(height * sy), &bitmap) || !bitmap.pixels || !bitmap.width || !bitmap.height || bitmap.width > 1024 || bitmap.height > 1024) return error(diagnostic, OSG_WIDGET_DRAW_ASSET, layer->id, 0, 0);
        }
        /* A narrow damaged rectangle can contain many rows in the same LVGL
         * pixel buffer. Bound alpha strips independently of that outer clip. */
        for(int32_t row=visible.y1;row<=visible.y2;row+=OSG_WIDGET_DRAW_ROWS) {
            const lv_area_t clip={visible.x1,row,visible.x2,LV_MIN(visible.y2,row+OSG_WIDGET_DRAW_ROWS-1)};
            const size_t rows=(size_t)lv_area_get_height(&clip);
            lv_layer_t *draw = target;
            const uint8_t opacity = (uint8_t)lround(paint->opacity * 255);
            if (opacity < 255) {
                lv_mem_monitor_t memory; lv_mem_monitor(&memory); const size_t required = columns * rows * 4 + 8192;
                if (memory.free_biggest_size < required) return error(diagnostic, OSG_WIDGET_DRAW_BUDGET, layer->id, required, memory.free_biggest_size);
                draw = lv_draw_layer_create(target, LV_COLOR_FORMAT_ARGB8888, &clip);
                if (!draw || !lv_draw_layer_alloc_buf(draw)) return error(diagnostic, OSG_WIDGET_DRAW_BUDGET, layer->id, required, memory.free_biggest_size);
            }
            const lv_area_t previous = draw->_clip_area; draw->_clip_area = clip;
            bool ok = true;
            if (layer->kind == OSG_LAYER_TEXT) {
                const lv_area_t box = {(int32_t)lround(left), (int32_t)lround(top), (int32_t)lround(left + width * sx) - 1, (int32_t)lround(top + height * sy) - 1};
                const lv_text_align_t aligns[] = {LV_TEXT_ALIGN_LEFT, LV_TEXT_ALIGN_CENTER, LV_TEXT_ALIGN_RIGHT};
                ok = osg_widget_text_draw(fonts, draw, box, text, (osg_widget_font)layer->font, layer->font_size, aligns[layer->align], sx, sy, paint->fill == OSG_WIDGET_SOURCE_NO_COLOR ? 0xffffff : paint->fill, 255);
            } else {
                const shape s = {layer, paint, width, height, osg_widget_gauge_fraction(layer, context), osg_widget_indicator_active(layer, context)};
                for (int y = clip.y1; y <= clip.y2; y++) {
                    uint32_t last = OSG_WIDGET_SOURCE_NO_COLOR; uint8_t last_alpha = 0; int start = clip.x1;
                    for (int x = clip.x1; x <= clip.x2 + 1; x++) {
                        uint32_t color = OSG_WIDGET_SOURCE_NO_COLOR; uint8_t alpha = 255;
                        if (x <= clip.x2) {
                            const double local_x = (x + .5 - left) / sx, local_y = (y + .5 - top) / sy;
                            if (layer->kind == OSG_LAYER_IMAGE) {
                                const unsigned ix = (unsigned)fmin(bitmap.width - 1, floor(local_x / width * bitmap.width)), iy = (unsigned)fmin(bitmap.height - 1, floor(local_y / height * bitmap.height));
                                const unsigned char *pixel = bitmap.pixels + ((size_t)iy * bitmap.width + ix) * 3; const uint16_t rgb = (uint16_t)(pixel[0] | pixel[1] << 8);
                                color = (uint32_t)((rgb >> 11) * 255 / 31) << 16 | (uint32_t)(((rgb >> 5) & 63) * 255 / 63) << 8 | (rgb & 31) * 255 / 31; alpha = pixel[2];
                            } else color = shape_pixel(&s, local_x, local_y);
                        }
                        if (x == clip.x1) { start = x; last = color; last_alpha = alpha; }
                        else if (color != last || alpha != last_alpha || x > clip.x2) { span(draw, start, x - 1, y, last, last_alpha); start = x; last = color; last_alpha = alpha; }
                    }
                }
            }
            draw->_clip_area = previous;
            if (draw != target) {
                lv_draw_image_dsc_t d; lv_draw_image_dsc_init(&d); d.src = draw; d.opa = opacity; lv_draw_layer(target, &d, &clip);
                /* Finish and free this strip before allocating the next layer. */
                while (target->draw_task_head) lv_draw_dispatch();
            }
            if (!ok) return error(diagnostic, OSG_WIDGET_DRAW_TEXT, layer->id, 0, 0);
            }
    }
    return true;
}
