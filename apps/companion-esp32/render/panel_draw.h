#ifndef OSG_PANEL_DRAW_H
#define OSG_PANEL_DRAW_H
#include "widget_draw.h"

typedef struct {
    osg_widget_draw_error code;
    char widget[65], layer[65], field[65];
    size_t required, available;
} osg_panel_draw_diagnostic;

typedef struct {
    osg_lvgl_backend backend;
    osg_widget_fonts fonts;
    osg_raster diagnostic_font;
    osg_runtime *frame;
    lv_layer_t *layer;
    uint64_t now_ms;
    osg_panel_draw_diagnostic diagnostic;
} osg_panel_draw;

/* One display-task owner. The caller pins the immutable panel and its prepared
 * image pixels until this synchronous operation, including present(), returns. */
bool osg_panel_draw_init(osg_panel_draw *, osg_board, void *buffer, size_t bytes);
void osg_panel_draw_destroy(osg_panel_draw *);
/* No board writes. Candidate admission checks every authored appearance;
 * per-frame checks resolve current feedback before invalidating any pixels. */
bool osg_panel_draw_check(osg_panel_draw *, const osg_runtime *, uint64_t now_ms, bool all_states);
bool osg_panel_draw_render(osg_panel_draw *, osg_runtime *, osg_damage, uint64_t now_ms);
#endif
