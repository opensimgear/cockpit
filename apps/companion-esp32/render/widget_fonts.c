#include "widget_fonts.h"
#include <math.h>
#include <string.h>

#if LV_USE_OS != LV_OS_NONE || LV_DRAW_SW_DRAW_UNIT_CNT != 1
#error "Portable font bitmaps require the pinned single synchronous software draw unit"
#endif

extern const unsigned char sans_start[] __asm__("_binary_osg_sans_ttf_start");
extern const unsigned char sans_end[] __asm__("_binary_osg_sans_ttf_end");
extern const unsigned char mono_start[] __asm__("_binary_osg_mono_ttf_start");
extern const unsigned char mono_end[] __asm__("_binary_osg_mono_ttf_end");
extern const unsigned char sans_notice[] __asm__("_binary_OFL_Rajdhani_txt_start");
extern const unsigned char sans_notice_end[] __asm__("_binary_OFL_Rajdhani_txt_end");
extern const unsigned char mono_notice[] __asm__("_binary_OFL_IBMPlexMono_txt_start");
extern const unsigned char mono_notice_end[] __asm__("_binary_OFL_IBMPlexMono_txt_end");
typedef struct { const unsigned char *start, *end, *notice, *notice_end; } font_source;
static const font_source sources[] = {{sans_start, sans_end, sans_notice, sans_notice_end}, {mono_start, mono_end, mono_notice, mono_notice_end}};

static lv_font_t *font(osg_widget_fonts *fonts, osg_widget_font face, unsigned size) {
    if (!fonts || (face != OSG_FONT_SANS && face != OSG_FONT_MONO) || size < 8 || size > 128 || fonts->sequence == UINT64_MAX) return NULL;
    osg_widget_font_slot *oldest = &fonts->slots[0];
    for (unsigned i = 0; i < OSG_WIDGET_FONT_SLOTS; i++) {
        osg_widget_font_slot *slot = &fonts->slots[i];
        if (slot->font && slot->face == face && slot->size == size) { slot->used = ++fonts->sequence; return slot->font; }
        if (!slot->font || slot->used < oldest->used) oldest = slot;
    }
    const font_source *source = &sources[face];
    lv_font_t *created = lv_tiny_ttf_create_data_ex(source->start, (size_t)(source->end - source->start), (int32_t)size, LV_FONT_KERNING_NONE, 0);
    if (!created) return NULL;
    // Keep the corresponding OFL text embedded with each live firmware font.
    created->user_data = (void *)source;
    if (oldest->font) lv_tiny_ttf_destroy(oldest->font);
    *oldest = (osg_widget_font_slot){created, size, face, ++fonts->sequence}; return created;
}
void osg_widget_fonts_destroy(osg_widget_fonts *fonts) {
    if (!fonts) return;
    for (unsigned i = 0; i < OSG_WIDGET_FONT_SLOTS; i++) if (fonts->slots[i].font) lv_tiny_ttf_destroy(fonts->slots[i].font);
    memset(fonts, 0, sizeof(*fonts));
}
/* Strict single-line UTF-8; count the same UTF-16 units as source text limits. */
static bool decode(const char **text, uint32_t *code, unsigned *units) {
    const unsigned char *s = (const unsigned char *)*text; uint32_t c = *s++; unsigned extra;
    if (c < 0x80) extra = 0;
    else if (c >= 0xc2 && c <= 0xdf) { c &= 0x1f; extra = 1; }
    else if (c >= 0xe0 && c <= 0xef) { c &= 0x0f; extra = 2; }
    else if (c >= 0xf0 && c <= 0xf4) { c &= 7; extra = 3; }
    else return false;
    for (unsigned i = 0; i < extra; i++) { if ((*s & 0xc0) != 0x80) return false; c = c << 6 | (*s++ & 0x3f); }
    if (c < 32 || (extra == 2 && c < 0x800) || (extra == 3 && c < 0x10000) || (c >= 0xd800 && c <= 0xdfff) || c > 0x10ffff) return false;
    *units += c > 0xffff ? 2 : 1; if (*units > 120) return false;
    *code = c; *text = (const char *)s; return true;
}
static bool measure(lv_font_t *face, const char *text, osg_widget_text_metrics *metrics) {
    if (!text || !metrics) return false;
    unsigned units = 0, width = 0;
    while (*text) {
        uint32_t code; lv_font_glyph_dsc_t glyph;
        if (!decode(&text, &code, &units) || !lv_font_get_glyph_dsc(face, &glyph, code, 0) || glyph.is_placeholder || !glyph.gid.index) return false;
        width += glyph.adv_w;
    }
    *metrics = (osg_widget_text_metrics){width, face->line_height / 2.0 - face->base_line}; return true;
}
bool osg_widget_text_measure(osg_widget_fonts *fonts, osg_widget_font kind, unsigned size, const char *text, osg_widget_text_metrics *metrics) {
    lv_font_t *face = font(fonts, kind, size); return face && measure(face, text, metrics);
}
bool osg_widget_text_draw(osg_widget_fonts *fonts, lv_layer_t *layer, lv_area_t area, const char *text, osg_widget_font kind, unsigned size, lv_text_align_t align, double sx, double sy, uint32_t color, uint8_t opacity) {
    if (!layer || !isfinite(sx) || !isfinite(sy) || sx <= 0 || sy <= 0 || sx > 32 || sy > 32 || (align != LV_TEXT_ALIGN_LEFT && align != LV_TEXT_ALIGN_CENTER && align != LV_TEXT_ALIGN_RIGHT)) return false;
    lv_font_t *face = font(fonts, kind, size); osg_widget_text_metrics metrics;
    if (!face || !measure(face, text, &metrics)) return false;
    lv_area_t previous = layer->_clip_area;
    layer->_clip_area = (lv_area_t){LV_MAX(previous.x1, area.x1), LV_MAX(previous.y1, area.y1), LV_MIN(previous.x2, area.x2), LV_MIN(previous.y2, area.y2)};
    if (layer->_clip_area.x1 > layer->_clip_area.x2 || layer->_clip_area.y1 > layer->_clip_area.y2) { layer->_clip_area = previous; return true; }
    const double offset = align == LV_TEXT_ALIGN_CENTER ? (lv_area_get_width(&area) - metrics.width * sx) / 2 : align == LV_TEXT_ALIGN_RIGHT ? lv_area_get_width(&area) - metrics.width * sx : 0;
    const double baseline = area.y1 + lv_area_get_height(&area) / 2.0 + metrics.baseline * sy;
    unsigned units = 0, pen = 0; bool ok = true;
    while (*text) {
        uint32_t code; lv_font_glyph_dsc_t glyph;
        if (!decode(&text, &code, &units) || !lv_font_get_glyph_dsc(face, &glyph, code, 0)) { ok = false; break; }
        // TinyTTF includes one trailing bitmap row in box_h. Its raw raster
        // starts at the TrueType y1, so compensate that padding at the baseline.
        const int32_t x = (int32_t)lround(area.x1 + offset + ((double)pen + glyph.ofs_x) * sx), y = (int32_t)lround(baseline - (glyph.box_h + glyph.ofs_y - 1) * sy);
        const lv_area_t target = {x, y, x + (int32_t)lround(glyph.box_w * sx) - 1, y + (int32_t)lround(glyph.box_h * sy) - 1};
        pen += glyph.adv_w;
        /* Spaces and other advancing glyphs may have no bitmap. Passing an
         * empty image to LVGL's ARGB transform path can divide by zero. */
        if (!glyph.box_w || !glyph.box_h || target.x2 < target.x1 || target.y2 < target.y1) continue;
        if (target.x2 < layer->_clip_area.x1 || target.x1 > layer->_clip_area.x2 || target.y2 < layer->_clip_area.y1 || target.y1 > layer->_clip_area.y2) continue;
        const lv_draw_buf_t *bitmap = lv_font_get_glyph_bitmap(&glyph, NULL);
        if (!bitmap) { ok = false; break; }
        lv_draw_image_dsc_t d; lv_draw_image_dsc_init(&d); d.src = bitmap; d.recolor = lv_color_hex(color); d.opa = opacity;
        d.scale_x = (int32_t)lround(sx * LV_SCALE_NONE); d.scale_y = (int32_t)lround(sy * LV_SCALE_NONE); d.pivot = (lv_point_t){0, 0};
        lv_area_t source = {x, y, x + glyph.box_w - 1, y + glyph.box_h - 1};
        lv_draw_image(layer, &d, &source);
        /* With LV_OS_NONE, wait_for_finish is a no-op. A preceding alpha layer
         * can leave this image queued, so drain the single software draw unit
         * before releasing the borrowed TinyTTF bitmap. */
        while (layer->draw_task_head) lv_draw_dispatch();
        lv_font_glyph_release_draw_data(&glyph);
    }
    layer->_clip_area = previous; return ok;
}
