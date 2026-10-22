#ifndef OSG_PANEL_SOURCE_H
#define OSG_PANEL_SOURCE_H
#include "wire.h"
#include "widget_source.h"

typedef struct {
    size_t document_bytes, bitmap_references;
    uint16_t width, height;
} osg_panel_source_limits;
typedef enum {
    OSG_PANEL_SOURCE_OK, OSG_PANEL_SOURCE_INVALID, OSG_PANEL_SOURCE_BUDGET,
    OSG_PANEL_SOURCE_UNSUPPORTED, OSG_PANEL_SOURCE_IDENTITY
} osg_panel_source_error;
typedef struct {
    osg_panel_source_error code;
    char widget[65], definition[65], layer[65], field[65];
    size_t required, available;
} osg_panel_source_diagnostic;

/* Network-task candidate admission, with no input authority or device I/O.
 * Canonical source bytes retain their authenticated publication identity.
 * Result and all referenced data belong to caller-owned aligned arena memory;
 * the JSON reply may be freed immediately. Failure returns NULL and rewinds
 * counters without changing any earlier admitted object in the same arena.
 * The caller fences an inactive bank before reusing its arena. */
bool osg_panel_source_compile(const cJSON *publication, const char *expected_hash,
    osg_crypto, const osg_panel_source_limits *, osg_widget_arena *,
    osg_image **result, osg_panel_source_diagnostic *);
#endif
