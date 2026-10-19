#ifndef OSG_COMPANION_H
#define OSG_COMPANION_H
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define OSG_MAX_PAGES 4
#define OSG_MAX_WIDGETS 32
#define OSG_TEXT_BYTES 121
#define OSG_STALE_MS 1000

typedef enum { OSG_BUTTON, OSG_INDICATOR, OSG_TEXT, OSG_NUMERIC, OSG_GAUGE } osg_kind;
typedef struct { double x, y, width, height; } osg_bounds;
typedef struct { const char *id, *title; uint16_t width, height; } osg_page;
typedef struct {
    const char *id, *reference, *output_id, *label, *prefix, *suffix, *unit;
    osg_kind kind;
    uint8_t page, precision;
    osg_bounds bounds;
    uint16_t foreground, background, accent;
    double min, max;
} osg_widget;
typedef struct {
    const char *ui_id, *publication_hash;
    const osg_page *pages;
    const osg_widget *widgets;
    size_t page_count, widget_count;
} osg_panel;
/* Callbacks consume their arguments synchronously. Coordinates are clipped by
 * the core. Drivers own fonts, pixel format conversion, DMA completion and pins. */
typedef struct {
    void *context;
    uint16_t width, height;
    void (*fill)(void *, int x, int y, int width, int height, uint16_t rgb565);
    void (*text)(void *, int x, int y, int width, int height, const char *, uint16_t rgb565);
} osg_display;
typedef enum { OSG_REQUESTED, OSG_LOCAL, OSG_HOST_ACCEPTED, OSG_HOST_SUBMITTED, OSG_GAME_CONFIRMED } osg_provenance;
typedef enum { OSG_BOOL, OSG_NUMBER, OSG_STRING } osg_value_kind;
typedef struct {
    enum { OSG_FEEDBACK_AUTOMATIC, OSG_FEEDBACK_UNKNOWN, OSG_FEEDBACK_STALE, OSG_FEEDBACK_DISCONNECTED } source_status;
    bool available;
    osg_value_kind kind;
    osg_provenance provenance;
    uint64_t received_ms;
    uint32_t source_age_ms;
    bool boolean;
    double number;
    char text[OSG_TEXT_BYTES];
} osg_sample;
/* Synchronous, bounded event sink. Live use must enqueue locally; it must never
 * wait for network I/O. Session admission/arming belongs to the network layer. */
typedef bool (*osg_input_sink)(void *, const char *control_id, bool down, uint64_t sequence);
typedef enum { OSG_SCREEN_PANEL, OSG_SCREEN_UNPAIRED, OSG_SCREEN_NO_PANEL, OSG_SCREEN_DISCONNECTED, OSG_SCREEN_PANEL_ERROR } osg_screen_state;
typedef struct {
    osg_screen_state screen;
    const osg_panel *panel;
    osg_display display;
    osg_sample samples[OSG_MAX_WIDGETS];
    osg_input_sink sink;
    void *sink_context;
    uint64_t sequence;
    int active;
    uint8_t page;
    bool live_input, arming;
    bool armed, pointer_down, await_release, connected;
} osg_runtime;

bool osg_panel_valid(const osg_panel *panel);
bool osg_init(osg_runtime *, const osg_panel *, osg_display, osg_input_sink, void *);
void osg_disarm(osg_runtime *);
bool osg_preview_arm(osg_runtime *);
bool osg_live_arm(osg_runtime *);
bool osg_set_page(osg_runtime *, size_t page);
void osg_touch(osg_runtime *, bool down, int x, int y);
void osg_connected(osg_runtime *, bool connected);
bool osg_feedback(osg_runtime *, const char *channel, const osg_sample *, uint64_t now_ms);
/* Previous is a completed draw snapshot. Publications are immutable; the caller
 * must force full damage when replacing/reusing a publication storage slot. */
typedef struct { bool full; uint32_t widgets; } osg_damage;
osg_damage osg_render_damage(const osg_runtime *, const osg_runtime *previous, uint64_t now_ms, uint64_t previous_ms);
void osg_widget_rect(const osg_runtime *, size_t index, int *x, int *y, int *width, int *height);
void osg_render(osg_runtime *, uint64_t now_ms);
#endif
