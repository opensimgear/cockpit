#ifndef OSG_WIDGET_H
#define OSG_WIDGET_H
#include "companion.h"

/* Private resolved runtime data, shared by admission, interaction and drawing.
 * This header has no JSON, transport, board or LVGL dependency. */
#define OSG_WIDGET_SOURCE_STATES 8
#define OSG_WIDGET_SOURCE_PORTS 8
#define OSG_WIDGET_SOURCE_NO_COLOR UINT32_MAX
typedef enum { OSG_WIDGET_NORMAL, OSG_WIDGET_ACTIVE, OSG_WIDGET_PRESSED, OSG_WIDGET_WARNING, OSG_WIDGET_STALE, OSG_WIDGET_UNKNOWN, OSG_WIDGET_DISABLED, OSG_WIDGET_DISCONNECTED } osg_widget_state;
typedef enum { OSG_LAYER_RECTANGLE, OSG_LAYER_ELLIPSE, OSG_LAYER_LINE, OSG_LAYER_TEXT, OSG_LAYER_IMAGE, OSG_LAYER_INDICATOR, OSG_LAYER_GAUGE } osg_layer_kind;
typedef enum { OSG_INTERACTION_NONE, OSG_INTERACTION_MOMENTARY, OSG_INTERACTION_TOGGLE, OSG_INTERACTION_SELECTOR, OSG_INTERACTION_NAVIGATE } osg_interaction_kind;
typedef struct {
    uint32_t fill, stroke;
    double stroke_width, opacity;
    const char *text;
    bool visible, has_stroke_width;
} osg_widget_paint;
typedef struct {
    const char *id, *asset_id, *unit;
    osg_layer_kind kind;
    osg_bounds bounds;
    double radius, min, max;
    uint8_t font, font_size, align, precision, orientation;
    int8_t output;
    osg_widget_paint paints[OSG_WIDGET_SOURCE_STATES];
} osg_widget_layer;
typedef struct {
    const char *name, *unit;
    osg_value_kind kind;
    bool has_unit, has_range;
    double min, max;
} osg_widget_port;
typedef struct {
    const char *id, *title, *input_name, *navigate_page;
    uint16_t version, width, height;
    osg_bounds hit_area;
    osg_interaction_kind interaction;
    double fallback, options[16];
    const char *option_labels[16];
    uint8_t option_count, output_count, layer_count;
    bool bipolar;
    int8_t confirmed_output, warning_output, enabled_output;
    osg_widget_port outputs[OSG_WIDGET_SOURCE_PORTS];
    osg_widget_layer *layers;
} osg_widget_component;
typedef struct { const char *id, *channel; uint8_t slot; } osg_bound_output;
typedef struct {
    const char *layer_id, *asset_id, *hash;
    osg_bounds bounds;
    const unsigned char *pixels;
    uint16_t width, height;
} osg_bound_bitmap;
struct osg_widget_binding {
    const osg_widget_component *definition;
    osg_bound_output outputs[OSG_WIDGET_SOURCE_PORTS];
    osg_bound_bitmap *bitmaps;
    uint8_t bitmap_count;
};
typedef struct { const char *id, *label; uint8_t page; uint32_t members; } osg_widget_group;
struct osg_panel_metadata { const osg_widget_group *groups; size_t group_count; };
typedef struct {
    const char *id, *channel, *unit;
    osg_value_kind kind;
    bool has_unit, has_range, portable_text;
    double min, max;
} osg_output_contract;

bool osg_widget_text_valid(const char *);
/* Validate the entire 256-unit wire value, then retain the shared 120-unit
 * display prefix without splitting a Unicode code point. */
bool osg_widget_feedback_text(const char *, char *, size_t capacity);
bool osg_widget_text_blank(const char *);
bool osg_widget_id_valid(const char *);
bool osg_widget_input(const osg_widget *, osg_value_kind *, double *fallback);
bool osg_panel_output(const osg_panel *, size_t slot, osg_output_contract *);
#endif
