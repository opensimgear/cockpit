#include "companion.h"
#include "widget.h"
#include "widget_source.h"
#include "widget_state.h"
#include "wire.h"
#include "input.h"
#include "panel_source.h"
#include <assert.h>
#include <math.h>
#include <openssl/sha.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static osg_runtime component_runtime;
static struct { const char *id; osg_value_kind kind; double value; } changes[128];
static size_t change_count;
static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static const char *str(const cJSON *v, const char *key) { return cJSON_GetStringValue(get(v, key)); }
static double number(const cJSON *v, const char *key) { const cJSON *field = get(v, key); assert(cJSON_IsNumber(field)); return field->valuedouble; }
static bool same(const char *a, const char *b) { return a && b && !strcmp(a, b); }
static bool source_sha(const void *p, size_t n, char out[72]) { unsigned char digest[32]; SHA256(p, n, digest); memcpy(out, "sha256:", 7); for (unsigned i = 0; i < 32; i++) (void)snprintf(out + 7 + i * 2, 3, "%02x", digest[i]); return true; }
static void fill(void *p, int x, int y, int w, int h, uint16_t c) { (void)p; (void)x; (void)y; (void)w; (void)h; (void)c; }
static void text(void *p, int x, int y, int w, int h, const char *s, uint16_t c) { (void)s; fill(p, x, y, w, h, c); }
static bool record(void *p, const char *id, osg_value_kind kind, double value, uint64_t sequence) {
    (void)p; assert(sequence == change_count + 1 && change_count < 128);
    changes[change_count].id = id; changes[change_count].kind = kind; changes[change_count++].value = value; return true;
}
static cJSON *load(const char *path) {
    FILE *f = fopen(path, "rb"); assert(f); assert(!fseek(f, 0, SEEK_END)); const long size = ftell(f); assert(size > 0 && size < 2 * 1024 * 1024); rewind(f);
    char *bytes = malloc((size_t)size + 1); assert(bytes && fread(bytes, 1, (size_t)size, f) == (size_t)size); bytes[size] = 0; fclose(f);
    cJSON *result = cJSON_Parse(bytes); free(bytes); assert(result); return result;
}
static osg_live_session session(const osg_panel *panel) {
    osg_live_session s = {0}; strcpy(s.client, "client"); strcpy(s.session, "session"); strcpy(s.generation, "1"); strcpy(s.instance, "instance"); strcpy(s.publication, panel->publication_hash); return s;
}
static void point(osg_runtime *r, size_t index, bool down) {
    int x, y, width, height; osg_widget_rect(r, index, &x, &y, &width, &height); osg_touch(r, down, x + width / 2, y + height / 2);
}
static size_t widget_index(const osg_panel *panel, const char *id) { size_t i = 0; while (i < panel->widget_count && !same(panel->widgets[i].id, id)) i++; assert(i < panel->widget_count); return i; }
static void compare(osg_runtime *r, const cJSON *expected) {
    assert(r->armed == cJSON_IsTrue(get(expected, "armed"))); assert(same(r->panel->pages[r->page].id, str(expected, "page")));
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        const osg_widget *w = &r->panel->widgets[i]; osg_value_kind kind; double fallback;
        const char *states[] = {"normal", "active", "pressed", "warning", "stale", "unknown", "disabled", "disconnected"};
        osg_widget_state_context appearance; osg_widget_runtime_context(r, i, r->now_ms, &appearance);
        assert(same(str(get(expected, "states"), w->id), states[osg_widget_state_resolve(w->component->definition, &appearance)]));
        if (osg_widget_input(w, &kind, &fallback)) {
            const cJSON *v = get(get(expected, "values"), w->reference); assert(v);
            assert(r->requested[i] == (kind == OSG_BOOL ? cJSON_IsTrue(v) : v->valuedouble));
            osg_widget_state_context context; osg_widget_runtime_context(r, i, r->now_ms, &context);
            if (r->requested[i] != fallback && w->component->definition->confirmed_output >= 0) assert(osg_widget_state_resolve(w->component->definition, &context) != OSG_WIDGET_ACTIVE);
        }
    }
    const cJSON *events = get(expected, "changes"); assert(change_count == (size_t)cJSON_GetArraySize(events));
    for (size_t i = 0; i < change_count; i++) {
        const cJSON *event = cJSON_GetArrayItem(events, (int)i), *value = get(event, "value"); assert(same(changes[i].id, str(event, "controlId")));
        assert(changes[i].kind == (cJSON_IsBool(value) ? OSG_BOOL : OSG_NUMBER)); assert(changes[i].value == (cJSON_IsBool(value) ? cJSON_IsTrue(value) : value->valuedouble));
    }
    const bool physical_input = r->active >= 0 && r->panel->widgets[r->active].component->definition->interaction != OSG_INTERACTION_NAVIGATE;
    assert(cJSON_GetArraySize(get(expected, "pressed")) == (physical_input ? 1 : 0));
    if (physical_input) assert(same(r->panel->widgets[r->active].id, cJSON_GetStringValue(cJSON_GetArrayItem(get(expected, "pressed"), 0))));
}
static cJSON *ack_frame(const osg_runtime *r, unsigned widget, bool value) {
    const osg_bound_output *port = &r->panel->widgets[widget].component->outputs[0];
    cJSON *ack = cJSON_CreateObject(), *f = cJSON_AddObjectToObject(ack, "feedback"); assert(f);
    cJSON_AddBoolToObject(ack, "ok", true); cJSON_AddStringToObject(ack, "type", "ack"); cJSON_AddStringToObject(ack, "clientId", "client"); cJSON_AddStringToObject(ack, "sessionId", "session"); cJSON_AddStringToObject(ack, "generation", "1"); cJSON_AddStringToObject(ack, "sequence", "1"); cJSON_AddStringToObject(ack, "ackStage", "source-accepted"); cJSON_AddBoolToObject(ack, "submitted", false);
    cJSON_AddNumberToObject(f, "version", 1); cJSON_AddStringToObject(f, "type", "feedback"); cJSON_AddStringToObject(f, "clientId", "client"); cJSON_AddStringToObject(f, "sessionId", "session"); cJSON_AddStringToObject(f, "generation", "1"); cJSON_AddStringToObject(f, "publicationHash", r->panel->publication_hash); cJSON_AddStringToObject(f, "controllerInstanceId", "instance"); cJSON_AddStringToObject(f, "sequence", "1"); cJSON_AddStringToObject(f, "bindingGeneration", "1"); cJSON_AddStringToObject(f, "mode", "snapshot");
    cJSON *bindings = cJSON_AddArrayToObject(f, "bindings"), *b = cJSON_CreateObject(); cJSON_AddItemToArray(bindings, b); cJSON_AddStringToObject(b, "outputId", port->id); cJSON_AddStringToObject(b, "channelId", port->channel);
    cJSON *values = cJSON_AddArrayToObject(f, "values"), *v = cJSON_CreateObject(); cJSON_AddItemToArray(values, v); cJSON_AddStringToObject(v, "outputId", port->id); cJSON_AddStringToObject(v, "channelId", port->channel);
    const osg_widget_port *contract = &r->panel->widgets[widget].component->definition->outputs[0];
    cJSON_AddBoolToObject(v, "value", value); cJSON_AddStringToObject(v, "status", "fresh"); cJSON_AddStringToObject(v, "unit", contract->has_unit && !osg_widget_text_blank(contract->unit) ? contract->unit : "ratio"); cJSON_AddStringToObject(v, "provenance", "game-confirmed"); cJSON_AddStringToObject(v, "sourceKind", "simulator"); cJSON_AddNumberToObject(v, "sourceAgeMs", 0); return ack;
}
static cJSON *value_at(cJSON *ack) { return cJSON_GetArrayItem(get(get(ack, "feedback"), "values"), 0); }
static void feedback_checks(osg_runtime *r) {
    osg_disarm(r); r->typed_sink = NULL; r->sink = NULL;
    osg_live_session s = session(r->panel); cJSON *good = ack_frame(r, 0, true);
    assert(osg_ack(good, &s, "1", r, 100));
    const unsigned slot = r->panel->widgets[0].component->outputs[0].slot;
    assert(r->samples[slot].received && r->samples[slot].simulator && r->samples[slot].boolean);
    osg_widget_state_context context; osg_widget_runtime_context(r, 0, 100, &context); assert(osg_widget_state_resolve(r->panel->widgets[0].component->definition, &context) == OSG_WIDGET_DISABLED);
    /* A valid first value followed by an invalid second value cannot partially
     * update a snapshot or its binding identities. */
    cJSON *atomic = cJSON_Duplicate(good, true), *atomic_frame = cJSON_GetObjectItemCaseSensitive(atomic, "feedback");
    const osg_bound_output *second = &r->panel->widgets[1].component->outputs[0];
    cJSON *second_binding = cJSON_CreateObject(); cJSON_AddStringToObject(second_binding, "outputId", second->id); cJSON_AddStringToObject(second_binding, "channelId", second->channel); cJSON_AddItemToArray((cJSON *)get(atomic_frame, "bindings"), second_binding);
    cJSON *second_value = cJSON_Duplicate(value_at(atomic), true); cJSON_ReplaceItemInObjectCaseSensitive(second_value, "outputId", cJSON_CreateString(second->id)); cJSON_ReplaceItemInObjectCaseSensitive(second_value, "channelId", cJSON_CreateString(second->channel)); cJSON_ReplaceItemInObjectCaseSensitive(second_value, "value", cJSON_CreateNumber(2)); cJSON_AddItemToArray((cJSON *)get(atomic_frame, "values"), second_value);
    cJSON_ReplaceItemInObjectCaseSensitive(value_at(atomic), "value", cJSON_CreateBool(false)); cJSON_ReplaceItemInObjectCaseSensitive(atomic_frame, "sequence", cJSON_CreateString("2"));
    assert(!osg_ack(atomic, &s, "1", r, 150) && r->samples[slot].boolean && !r->samples[second->slot].received); cJSON_Delete(atomic);
    for (unsigned mutation = 0; mutation < 16; mutation++) {
        cJSON *bad = cJSON_Duplicate(good, true), *f = cJSON_GetObjectItemCaseSensitive(bad, "feedback"), *v = value_at(bad);
        cJSON_ReplaceItemInObjectCaseSensitive(f, "sequence", cJSON_CreateString("2"));
        switch (mutation) {
            case 0: cJSON_ReplaceItemInObjectCaseSensitive(f, "clientId", cJSON_CreateString("other")); break;
            case 1: cJSON_ReplaceItemInObjectCaseSensitive(f, "controllerInstanceId", cJSON_CreateString("other")); break;
            case 2: cJSON_ReplaceItemInObjectCaseSensitive(f, "generation", cJSON_CreateString("2")); break;
            case 3: cJSON_ReplaceItemInObjectCaseSensitive(v, "outputId", cJSON_CreateString("other")); break;
            case 4: cJSON_ReplaceItemInObjectCaseSensitive(v, "channelId", cJSON_CreateString("other")); break;
            case 5: cJSON_ReplaceItemInObjectCaseSensitive(v, "sourceKind", cJSON_CreateString("host")); break;
            case 6: cJSON_ReplaceItemInObjectCaseSensitive(v, "value", cJSON_CreateNumber(2)); break;
            case 7: cJSON_ReplaceItemInObjectCaseSensitive(v, "value", cJSON_CreateNull()); break;
            case 8: cJSON_ReplaceItemInObjectCaseSensitive(v, "sourceAgeMs", cJSON_CreateNumber(-1)); break;
            case 9: cJSON_AddItemToArray((cJSON *)get(f, "values"), cJSON_Duplicate(v, true)); break;
            case 10: cJSON_AddItemToArray((cJSON *)get(f, "bindings"), cJSON_Duplicate(cJSON_GetArrayItem(get(f, "bindings"), 0), true)); break;
            case 11: cJSON_ReplaceItemInObjectCaseSensitive(f, "mode", cJSON_CreateString("delta")); cJSON_DeleteItemFromObjectCaseSensitive(f, "bindings"); cJSON_ReplaceItemInObjectCaseSensitive(f, "bindingGeneration", cJSON_CreateString("2")); break;
            case 12: cJSON_ReplaceItemInObjectCaseSensitive(f, "sequence", cJSON_CreateString("0")); break;
            case 13: cJSON_ReplaceItemInObjectCaseSensitive(v, "sourceAgeMs", cJSON_CreateNumber(86400001)); break;
            case 14: cJSON_ReplaceItemInObjectCaseSensitive(v, "value", cJSON_CreateObject()); break;
            case 15: cJSON_AddObjectToObject(v, "range"); break;
        }
        osg_sample *before = malloc(sizeof(r->samples)); assert(before); memcpy(before, r->samples, sizeof(r->samples)); const osg_live_session prior = s;
        assert(!osg_ack(bad, &s, "1", r, 200)); assert(!memcmp(before, r->samples, sizeof(r->samples)) && !memcmp(&prior, &s, sizeof(s))); free(before); cJSON_Delete(bad);
    }
    cJSON *duplicate = cJSON_Duplicate(good, true); cJSON_ReplaceItemInObjectCaseSensitive(value_at(duplicate), "value", cJSON_CreateBool(false)); assert(osg_ack(duplicate, &s, "1", r, 500)); assert(r->samples[slot].boolean && r->samples[slot].received_ms == 100); cJSON_Delete(duplicate);
    cJSON *delta = cJSON_Duplicate(good, true), *f = cJSON_GetObjectItemCaseSensitive(delta, "feedback");
    cJSON_ReplaceItemInObjectCaseSensitive(f, "sequence", cJSON_CreateString("2")); cJSON_ReplaceItemInObjectCaseSensitive(f, "mode", cJSON_CreateString("delta")); cJSON_DeleteItemFromObjectCaseSensitive(f, "bindings"); cJSON_ReplaceItemInObjectCaseSensitive(value_at(delta), "value", cJSON_CreateBool(false)); assert(osg_ack(delta, &s, "1", r, 600)); assert(!r->samples[slot].boolean); cJSON_Delete(delta);
    cJSON *utf8 = ack_frame(r, 14, true); f = cJSON_GetObjectItemCaseSensitive(utf8, "feedback"); cJSON_ReplaceItemInObjectCaseSensitive(f, "bindingGeneration", cJSON_CreateString("2"));
    char value[515]; for (unsigned i = 0; i < 257; i++) { value[i * 2] = (char)0xc3; value[i * 2 + 1] = (char)0xa9; } value[240] = 0;
    cJSON_ReplaceItemInObjectCaseSensitive(value_at(utf8), "value", cJSON_CreateString(value)); assert(osg_ack(utf8, &s, "1", r, 700));
    const unsigned text_slot = r->panel->widgets[14].component->outputs[0].slot; assert(strlen(r->samples[text_slot].text) == 240 && !r->samples[slot].received);
    value[240] = (char)0xc3; value[512] = 0; cJSON_ReplaceItemInObjectCaseSensitive(value_at(utf8), "value", cJSON_CreateString(value)); cJSON_ReplaceItemInObjectCaseSensitive(f, "sequence", cJSON_CreateString("2")); assert(osg_ack(utf8, &s, "1", r, 700)); assert(strlen(r->samples[text_slot].text) == 240);
    value[512] = (char)0xc3; value[514] = 0; cJSON_ReplaceItemInObjectCaseSensitive(value_at(utf8), "value", cJSON_CreateString(value)); cJSON_ReplaceItemInObjectCaseSensitive(f, "sequence", cJSON_CreateString("3")); assert(!osg_ack(utf8, &s, "1", r, 800));
    value[242] = 0; cJSON_ReplaceItemInObjectCaseSensitive(value_at(utf8), "value", cJSON_CreateString("Valid")); cJSON_ReplaceItemInObjectCaseSensitive(value_at(utf8), "unit", cJSON_CreateString(value)); assert(!osg_ack(utf8, &s, "1", r, 800));
    osg_runtime *previous = malloc(sizeof(*previous)); assert(previous); *previous = *r;
    assert(!osg_render_damage(r, previous, 1699, 700).widgets);
    assert(osg_render_damage(r, previous, 1700, 700).widgets == (UINT32_C(1) << 14));
    strcpy(r->samples[text_slot].text, "Changed"); assert(osg_render_damage(r, previous, 700, 700).widgets == (UINT32_C(1) << 14)); free(previous);
    cJSON_Delete(utf8); cJSON_Delete(good);
}
static void queue_checks(osg_runtime *r) {
    const osg_panel *panel = r->panel;
    assert(osg_init(r, panel, r->display, NULL, NULL)); osg_connected(r, true); assert(osg_set_page(r, 1));
    osg_input queue = {0}; osg_input_bind(&queue, r, true, 100);
    osg_input_poll(&queue, true, false, 0, 0, 100); osg_input_render(&queue, panel, true, true, 100);
    uint64_t epoch; assert(osg_input_prepare(&queue, 100, &epoch) && osg_input_commit(&queue, epoch, 100, 100));
    int x, y, width, height; osg_panel_widget_rect(panel, 13, 800, 480, &x, &y, &width, &height);
    for (unsigned i = 0; i < 3; i++) {
        osg_input_poll(&queue, true, true, x + width / 2, y + height / 2, 101 + i);
        osg_input_poll(&queue, true, false, 0, 0, 101 + i);
        osg_input_edge edge; size_t count; uint64_t observed;
        assert(osg_input_peek(&queue, &edge, 1, &count, &observed, 101 + i) && count == 1 && observed == epoch);
        assert(edge.widget == 13 && edge.kind == OSG_NUMBER && edge.number == (i == 0 ? 0 : i == 1 ? 1 : -1));
        assert(osg_input_accept(&queue, epoch, count));
    }
    assert(!r->typed_sink(r->sink_context, panel->widgets[13].reference, OSG_BOOL, 1, 4));
    assert(queue.mode == OSG_INPUT_BLOCKED && !r->armed && r->requested[13] == -1);
    r->sink = NULL; r->typed_sink = NULL; r->sink_context = NULL;
}
static void binding_checks(osg_panel *panel, osg_widget *widgets) {
    osg_widget_binding *first = (osg_widget_binding *)widgets[0].component, *second = (osg_widget_binding *)widgets[1].component;
    const unsigned original_slot = first->outputs[0].slot;
    first->outputs[0].slot = OSG_MAX_WIDGETS; assert(!osg_panel_valid(panel)); first->outputs[0].slot = (uint8_t)original_slot;
    const unsigned second_slot = second->outputs[0].slot;
    second->outputs[0].slot = (uint8_t)original_slot; assert(!osg_panel_valid(panel)); second->outputs[0].slot = (uint8_t)second_slot;
    const char *second_id = second->outputs[0].id;
    second->outputs[0].id = first->outputs[0].id; assert(!osg_panel_valid(panel)); second->outputs[0].id = second_id;
    widgets[0].component = NULL; assert(!osg_panel_valid(panel)); widgets[0].component = first;
    const char *reference = widgets[1].reference;
    widgets[1].reference = widgets[0].reference; assert(!osg_panel_valid(panel)); widgets[1].reference = reference;
    const osg_bounds before = widgets[1].bounds; widgets[1].bounds = widgets[0].bounds; assert(!osg_panel_valid(panel)); widgets[1].bounds = before;
    const osg_bounds frame = widgets[7].bounds; widgets[7].bounds = (osg_bounds){0, 0, 1, 1}; assert(osg_panel_valid(panel)); widgets[7].bounds = frame;
    osg_widget_group groups[2] = {{"group", "Controls", 0, 1}, {"other", "Other", 0, 0}};
    osg_panel_metadata metadata = {groups, 2}; panel->metadata = &metadata; assert(osg_panel_valid(panel));
    metadata.group_count = 65; assert(!osg_panel_valid(panel)); metadata.group_count = 2;
    metadata.groups = NULL; assert(!osg_panel_valid(panel)); metadata.groups = groups;
    groups[0].id = "!bad"; assert(!osg_panel_valid(panel)); groups[0].id = "group";
    groups[0].label = NULL; assert(!osg_panel_valid(panel)); groups[0].label = " "; assert(!osg_panel_valid(panel)); groups[0].label = "Controls";
    groups[0].page = 2; assert(!osg_panel_valid(panel)); groups[0].page = 0;
    groups[0].members = UINT32_C(1) << 31; assert(!osg_panel_valid(panel)); groups[0].members = UINT32_C(1) << 12; assert(!osg_panel_valid(panel)); groups[0].members = 1;
    groups[1].id = "group"; assert(!osg_panel_valid(panel)); groups[1].id = "other";
    metadata.group_count = 0; metadata.groups = NULL; assert(osg_panel_valid(panel)); panel->metadata = NULL;
    assert(!osg_widget_text_valid("\xc0\x80") && !osg_widget_text_valid("\xed\xa0\x80") && !osg_widget_text_valid("\xf4\x90\x80\x80") && !osg_widget_text_valid("\xe2\x82"));
    assert(osg_widget_text_valid("é —") && osg_widget_text_blank(" \xc2\xa0") && !osg_widget_text_blank("Text") && !osg_widget_text_blank(NULL));
    char wire[1025], display[OSG_TEXT_BYTES]; memset(wire, 'A', 256); wire[256] = 0;
    assert(osg_widget_feedback_text(wire, display, sizeof(display)) && strlen(display) == 120);
    wire[256] = 'A'; wire[257] = 0; assert(!osg_widget_feedback_text(wire, display, sizeof(display)));
    wire[200] = 1; wire[201] = 0; assert(!osg_widget_feedback_text(wire, display, sizeof(display)));
    memset(wire, 'A', 119); memcpy(wire + 119, "😀tail", sizeof("😀tail")); assert(osg_widget_feedback_text(wire, display, sizeof(display)) && strlen(display) == 119);
    assert(!osg_widget_feedback_text(NULL, display, sizeof(display)) && !osg_widget_feedback_text("text", NULL, 4) && !osg_widget_feedback_text("é", display, 2));
}
void component_tests(void) {
    cJSON *fixture = load("apps/companion-esp32/test/widget-input.json");
    unsigned char *memory = malloc(192 * 1024); assert(memory); osg_widget_arena arena = {.data = memory, .capacity = 192 * 1024, .max_layers = 128, .max_components = 32};
    cJSON *publication = cJSON_CreateObject(); assert(publication);
    cJSON_AddStringToObject(publication, "canonicalDocument", str(fixture, "canonicalDocument")); cJSON_AddStringToObject(publication, "publicationHash", str(fixture, "publicationHash")); cJSON_AddArrayToObject(publication, "assets");
    assert(osg_json_configure(512 * 1024, 192 * 1024, 8192));
    osg_scene *image; assert(osg_panel_source_compile(publication, str(fixture, "publicationHash"), (osg_crypto){.sha256 = source_sha}, &(osg_panel_source_limits){64 * 1024, 32, 800, 480}, &arena, &image, NULL)); cJSON_Delete(publication);
    osg_panel *panel = &image->panel;
    assert(osg_panel_valid(panel)); binding_checks(panel, image->widgets); osg_runtime *r = &component_runtime; change_count = 0;
    assert(osg_init(r, panel, (osg_display){.width = 800, .height = 480, .fill = fill, .text = text}, NULL, NULL)); r->typed_sink = record; osg_connected(r, true);
    osg_live_session live = session(panel); unsigned step = 0;
    for (const cJSON *action = get(fixture, "steps")->child; action; action = action->next, step++) {
        const char *type = str(action, "type"); const uint64_t now = (uint64_t)number(action, "now");
        if (same(type, "arm")) assert(osg_preview_arm(r));
        else if (same(type, "disarm")) osg_disarm(r);
        else if (same(type, "tick")) osg_tick(r, now);
        else if (same(type, "feedback")) { const cJSON *ack = get(action, "ack"); assert(osg_ack(ack, &live, str(ack, "sequence"), r, now)); osg_tick(r, now); }
        else if (same(type, "down")) point(r, widget_index(panel, str(action, "widgetId")), true);
        else if (same(type, "up")) osg_touch(r, false, 0, 0);
        else if (same(type, "out")) osg_touch(r, true, -1, -1);
        else abort();
        compare(r, get(action, "expected"));
    }
    printf("Portable component input trace: %u steps and %zu ordered changes\n", step, change_count);
    feedback_checks(r);
    queue_checks(r);
    assert(!osg_widget_input(NULL, NULL, NULL)); osg_widget_state_context context; osg_widget_runtime_context(NULL, 0, 0, &context); assert(!context.connected);
    assert(!osg_panel_output(NULL, 0, NULL));
    cJSON_Delete(fixture); free(memory); assert(osg_json_configure(OSG_JSON_HEAP_BYTES, OSG_REPLY_BYTES, 2048));
}
