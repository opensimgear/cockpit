#ifndef OSG_LVGL_BACKEND_H
#define OSG_LVGL_BACKEND_H
#include "board.h"
#include "lvgl.h"

#if LVGL_VERSION_MAJOR != 9 || LVGL_VERSION_MINOR != 4 || LVGL_VERSION_PATCH != 0
#error "Use the locked LVGL 9.4.0 backend"
#endif

typedef bool (*osg_lvgl_paint)(void *, lv_layer_t *);
#define OSG_LVGL_TRANSPARENT UINT32_MAX
typedef struct {
    osg_board board;
    lv_display_t *display;
    lv_obj_t *surface;
    osg_lvgl_paint paint;
    void *paint_context;
    /* Synchronous notification after a successful board write; the frame and
     * presentation fence still need to complete before display readiness. */
    void (*progress)(void *);
    void *progress_context;
    bool failed, rendering;
    uint32_t flushes;
} osg_lvgl_backend;

/* The caller owns the aligned partial RGB565 buffer throughout this lifetime.
 * All APIs are synchronous and belong to the one display task. Board writes
 * must consume bytes and fence DMA before returning, as in the existing board API.
 * No input/session ownership is created or restored here. */
bool osg_lvgl_init(osg_lvgl_backend *, osg_board, void *buffer, size_t bytes);
void osg_lvgl_destroy(osg_lvgl_backend *);
bool osg_lvgl_render(osg_lvgl_backend *, osg_lvgl_paint, void *context);
/* Absolute screen regions; NULL means full damage. A non-NULL empty list is a
 * successful no-op. Paint still replays the scene beneath each LVGL clip. */
bool osg_lvgl_render_regions(osg_lvgl_backend *, osg_lvgl_paint, void *context, const lv_area_t *, size_t count);

/* These are target drawing operations after source properties and state resolve.
 * Colors are RGB888; bounds and text are clipped to their caller-supplied area. */
void osg_lvgl_rect(lv_layer_t *, lv_area_t, uint32_t fill, uint32_t stroke, unsigned stroke_width, unsigned radius, uint8_t opacity);
void osg_lvgl_line(lv_layer_t *, lv_area_t, uint32_t stroke, unsigned width, uint8_t opacity);
void osg_lvgl_ellipse(lv_layer_t *, lv_area_t, uint32_t fill, uint32_t stroke, unsigned stroke_width, uint8_t opacity);
void osg_lvgl_text(lv_layer_t *, lv_area_t, const char *, const lv_font_t *, lv_text_align_t, uint32_t color, uint8_t opacity);
void osg_lvgl_arc(lv_layer_t *, lv_area_t, double fraction, uint32_t color, unsigned width, uint8_t opacity);
void osg_lvgl_image(lv_layer_t *, lv_area_t, const lv_image_dsc_t *, uint8_t opacity);
#endif
