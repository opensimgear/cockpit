#ifndef OSG_WIDGET_DRAW_H
#define OSG_WIDGET_DRAW_H
#include "lvgl_backend.h"
#include "widget_fonts.h"
#include "widget_state.h"

#ifndef OSG_WIDGET_DRAW_ROWS
#ifdef ESP_PLATFORM
#define OSG_WIDGET_DRAW_ROWS 4
#else
#define OSG_WIDGET_DRAW_ROWS 8
#endif
#endif
#define OSG_WIDGET_DRAW_TEXT_BYTES 512
typedef struct { const unsigned char *pixels; uint16_t width, height; } osg_widget_bitmap;
/* RGB565 little-endian plus alpha, reusing the verified device image cache.
 * The immutable bitmap stays pinned throughout the synchronous draw. */
typedef bool (*osg_widget_bitmap_lookup)(void *, const char *asset_id, unsigned width, unsigned height, osg_widget_bitmap *);
/* Lookup only prepared pixels in this immutable instance binding. */
bool osg_widget_bound_bitmap(void *binding, const char *asset_id, unsigned width, unsigned height, osg_widget_bitmap *);
typedef enum { OSG_WIDGET_DRAW_OK, OSG_WIDGET_DRAW_ARGUMENT, OSG_WIDGET_DRAW_BUDGET, OSG_WIDGET_DRAW_TEXT, OSG_WIDGET_DRAW_ASSET } osg_widget_draw_error;
typedef struct { osg_widget_draw_error code; const char *layer; size_t required, available; } osg_widget_draw_diagnostic;

/* Draw-task only. Coordinates use the portable SVG's independent x/y scaling.
 * Group alpha is applied once to each layer, including overlapping strokes and
 * glyphs. At most one partial ARGB8888 strip is needed; no full-widget bitmap.
 * The existing backend owns board writes, presentation and failure recovery. */
bool osg_widget_draw(lv_layer_t *, const osg_widget_component *, lv_area_t,
    const osg_widget_state_context *, osg_widget_fonts *, osg_widget_bitmap_lookup, void *, osg_widget_draw_diagnostic *);
/* Bounded live text projection. Numeric output uses exact binary64 rounding
 * with the shared 0–4 decimal places for magnitudes below 1e21. Larger values
 * fail explicitly, rather than displaying an approximate simulator value. */
bool osg_widget_layer_text(const osg_widget_layer *, const osg_widget_state_context *, osg_widget_state, char *, size_t);
#endif
