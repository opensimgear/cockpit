#ifndef OSG_WIDGET_SOURCE_H
#define OSG_WIDGET_SOURCE_H
#include "companion.h"
#include "cJSON.h"

/* Private, resolved runtime data, never a second authoring or wire format. */
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
typedef struct {
    unsigned char *data;
    size_t capacity, used, max_layers, max_components, layers, components;
} osg_widget_arena;
typedef enum { OSG_WIDGET_SOURCE_OK, OSG_WIDGET_SOURCE_INVALID, OSG_WIDGET_SOURCE_BUDGET } osg_widget_source_error;
typedef struct {
    osg_widget_source_error code;
    char definition[65], layer[65], field[65];
    size_t required, available;
} osg_widget_diagnostic;

/* Network-task admission only. Inputs are a portable WidgetDefinition, its
 * instance property overrides and the document's asset descriptors. The caller
 * owns aligned arena memory and supplies its board's explicit resource limits.
 * The result owns every string; cJSON may be freed immediately. Failure rewinds
 * the arena, returns no candidate and cannot mutate previously admitted objects.
 * This function grants no input authority and performs no rendering or I/O. */
bool osg_widget_source_compile(const cJSON *definition, const cJSON *properties, const cJSON *assets,
    osg_widget_arena *, const osg_widget_component **, osg_widget_diagnostic *);
#endif
