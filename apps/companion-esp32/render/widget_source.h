#ifndef OSG_WIDGET_SOURCE_H
#define OSG_WIDGET_SOURCE_H
#include "widget.h"
#include "cJSON.h"
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
