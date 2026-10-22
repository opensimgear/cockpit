#ifndef OSG_SCENE_BANK_H
#define OSG_SCENE_BANK_H
#include "panel_source.h"

typedef struct {
    void *context;
    void *(*allocate)(void *, size_t);
    void (*release)(void *, void *);
    size_t (*available)(void *);
} osg_scene_allocator;
typedef struct { size_t bytes, layers, components; } osg_scene_memory;
typedef struct { unsigned slot; uint64_t generation; } osg_scene_ticket;
typedef struct {
    osg_widget_arena arena;
    osg_scene *scene;
    uint64_t generation;
    bool prepared, checked;
} osg_scene_slot;
typedef struct {
    osg_scene_slot slots[2];
    osg_scene_allocator allocator;
    osg_scene_memory memory;
    unsigned active;
    bool active_valid;
} osg_scene_banks;
typedef enum { OSG_SCENE_REJECTED, OSG_SCENE_WAIT, OSG_SCENE_PREPARED } osg_scene_result;

/* Network task, under the existing state mutex when sharing flags/pointers with
 * the display task. Borrowed is the display task's current immutable panel.
 * Allocation/admission has no device I/O or input/session authority. */
bool osg_scene_banks_init(osg_scene_banks *, osg_scene_memory, osg_scene_allocator);
osg_scene_result osg_scene_banks_prepare(osg_scene_banks *, const osg_panel *borrowed,
    const cJSON *publication, const char *expected, osg_crypto,
    const osg_panel_source_limits *, osg_scene_ticket *, osg_scene **,
    osg_panel_source_diagnostic *);
const osg_scene *osg_scene_banks_candidate(const osg_scene_banks *, osg_scene_ticket);
/* Mark the exact candidate after display-task font/resource preflight. A stale
 * result cannot validate or activate a newer candidate. This never arms input. */
bool osg_scene_banks_checked(osg_scene_banks *, osg_scene_ticket, bool success);
bool osg_scene_banks_activate(osg_scene_banks *, osg_scene_ticket);
void osg_scene_banks_cancel(osg_scene_banks *, osg_scene_ticket);
/* Teardown only after network/display tasks and all readers have stopped. */
void osg_scene_banks_destroy(osg_scene_banks *);
#endif
