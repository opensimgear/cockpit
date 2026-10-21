#ifndef OSG_WIDGET_FONTS_H
#define OSG_WIDGET_FONTS_H
#include "lvgl.h"
#include <stdbool.h>

#define OSG_WIDGET_FONT_SLOTS 8
typedef enum { OSG_FONT_SANS, OSG_FONT_MONO } osg_widget_font;
typedef struct { lv_font_t *font; unsigned size; osg_widget_font face; uint64_t used; } osg_widget_font_slot;
typedef struct { osg_widget_font_slot slots[OSG_WIDGET_FONT_SLOTS]; uint64_t sequence; } osg_widget_fonts;
typedef struct { unsigned width; double baseline; } osg_widget_text_metrics;

/* Fixed trusted font assets, never untrusted font uploads. This display-task
 * cache owns at most eight TinyTTF faces/sizes; glyph bitmaps are not retained.
 * Call after LVGL initialization; zero-initialize the cache before first use. */
void osg_widget_fonts_destroy(osg_widget_fonts *);
bool osg_widget_text_measure(osg_widget_fonts *, osg_widget_font, unsigned size, const char *, osg_widget_text_metrics *);
/* Font sizes/advances belong to authored widget coordinates. Independent x/y
 * scaling matches the portable SVG viewBox, including non-square instances. */
bool osg_widget_text_draw(osg_widget_fonts *, lv_layer_t *, lv_area_t, const char *, osg_widget_font, unsigned size, lv_text_align_t, double scale_x, double scale_y, uint32_t color, uint8_t opacity);
#endif
