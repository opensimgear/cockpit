#include "scene_bank.h"
#include <stdio.h>
#include <string.h>

bool osg_scene_banks_init(osg_scene_banks *banks, osg_scene_memory memory, osg_scene_allocator allocator) {
    if (!banks || !memory.bytes || !memory.layers || !memory.components || !allocator.allocate || !allocator.release) return false;
    memset(banks, 0, sizeof(*banks)); banks->memory = memory; banks->allocator = allocator; return true;
}
static osg_scene_slot *candidate(osg_scene_banks *banks, osg_scene_ticket ticket) {
    if (!banks || ticket.slot > 1 || !ticket.generation) return NULL;
    osg_scene_slot *slot = &banks->slots[ticket.slot];
    if (slot->generation != ticket.generation || !slot->prepared || (banks->active_valid && banks->active == ticket.slot)) return NULL;
    return slot;
}
const osg_scene *osg_scene_banks_candidate(const osg_scene_banks *banks, osg_scene_ticket ticket) {
    if (!banks || ticket.slot > 1 || !ticket.generation) return NULL;
    const osg_scene_slot *slot = &banks->slots[ticket.slot];
    return slot->generation == ticket.generation && slot->prepared && !(banks->active_valid && banks->active == ticket.slot) ? slot->scene : NULL;
}
static osg_scene_result reject(osg_panel_source_diagnostic *error, osg_panel_source_error code, const char *field, size_t required, size_t available) {
    if (error) { memset(error, 0, sizeof(*error)); error->code = code; error->required = required; error->available = available; (void)snprintf(error->field, sizeof(error->field), "%s", field); }
    return OSG_SCENE_REJECTED;
}
osg_scene_result osg_scene_banks_prepare(osg_scene_banks *banks, const osg_panel *borrowed, const cJSON *publication, const char *expected, osg_crypto crypto,
    const osg_panel_source_limits *limits, osg_scene_ticket *ticket, osg_scene **out, osg_panel_source_diagnostic *error) {
    if (out) *out = NULL;
    if (ticket) *ticket = (osg_scene_ticket){0};
    if (error) memset(error, 0, sizeof(*error));
    if (!banks || !ticket || !out || !banks->allocator.allocate || !banks->allocator.release || !banks->memory.bytes)
        return reject(error, OSG_PANEL_SOURCE_INVALID, "sceneBank.arguments", 0, 0);
    const unsigned index = banks->active_valid ? 1 - banks->active : 0;
    osg_scene_slot *slot = &banks->slots[index];
    /* Keep scene even after cancellation: it is also the fence identity until
     * every display read of this storage generation has completed. */
    if (slot->scene && borrowed == &slot->scene->panel) return OSG_SCENE_WAIT;
    if (slot->generation == UINT64_MAX) return reject(error, OSG_PANEL_SOURCE_UNSUPPORTED, "sceneBank.generationExhausted", 0, 0);
    slot->generation++; slot->prepared = slot->checked = false;
    if (!slot->arena.data) {
        slot->arena.data = banks->allocator.allocate(banks->allocator.context, banks->memory.bytes);
        if (!slot->arena.data) return reject(error, OSG_PANEL_SOURCE_BUDGET, "sceneBank.bytes", banks->memory.bytes,
            banks->allocator.available ? banks->allocator.available(banks->allocator.context) : 0);
    }
    slot->arena.capacity = banks->memory.bytes; slot->arena.max_layers = banks->memory.layers; slot->arena.max_components = banks->memory.components;
    slot->arena.used = slot->arena.layers = slot->arena.components = 0; slot->scene = NULL;
    if (!osg_panel_source_compile(publication, expected, crypto, limits, &slot->arena, &slot->scene, error)) return OSG_SCENE_REJECTED;
    slot->prepared = true; *ticket = (osg_scene_ticket){index,slot->generation}; *out = slot->scene; return OSG_SCENE_PREPARED;
}
bool osg_scene_banks_checked(osg_scene_banks *banks, osg_scene_ticket ticket, bool success) {
    osg_scene_slot *slot = candidate(banks, ticket); if (!slot) return false;
    slot->checked = success;
    if (!success) slot->prepared = false;
    return true;
}
bool osg_scene_banks_activate(osg_scene_banks *banks, osg_scene_ticket ticket) {
    osg_scene_slot *slot = candidate(banks, ticket); if (!slot || !slot->checked) return false;
    banks->active = ticket.slot; banks->active_valid = true; slot->prepared = false; return true;
}
void osg_scene_banks_cancel(osg_scene_banks *banks, osg_scene_ticket ticket) {
    osg_scene_slot *slot = candidate(banks, ticket); if (slot) slot->prepared = slot->checked = false;
}
void osg_scene_banks_destroy(osg_scene_banks *banks) {
    if (!banks) return;
    for (unsigned i = 0; i < 2; i++) if (banks->slots[i].arena.data) banks->allocator.release(banks->allocator.context, banks->slots[i].arena.data);
    memset(banks, 0, sizeof(*banks));
}
