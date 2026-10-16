#ifndef OSG_RENDER_SCENE_H
#define OSG_RENDER_SCENE_H
#include "expressions.h"
#include "../core/raster.h"
#define OSG_SCENE_MAX_NODES 256u
#define OSG_SCENE_MAX_VARIABLES 64u
#define OSG_SCENE_MAX_TILES 741u

typedef struct { osg_render_read read; void *context; uint32_t bytes; } osg_scene_source;
typedef struct { uint16_t width,height; int16_t x,y; uint16_t content_width,content_height; const osg_render_package_limits *limits; uint16_t layers; uint32_t dirty_pixels,blend_pixels; } osg_scene_profile;
extern const osg_scene_profile OSG_SCENE_SUNTON, OSG_SCENE_WAVESHARE;
typedef struct { osg_expression_type type; bool available,fresh; double value; } osg_scene_feedback;
typedef struct { const osg_scene_feedback *feedback; uint16_t feedback_count; const bool *pressed; uint16_t pressed_count; bool connected; uint16_t page; uint64_t now_ms; } osg_scene_state;
typedef struct { int32_t number; int16_t dx,dy; uint16_t color,image; uint8_t visible,numbered; uint16_t reserved; } osg_scene_properties;
typedef struct { uint32_t pages,nodes,variables,bindings,hit_regions,glyphs,timelines,controls,max_page_draw_ops,max_update_instructions,max_dirty_pixels,max_layers_per_tile,max_blend_pixel_layers,minimum_snapshot_bytes,runtime_bytes; } osg_scene_usage;
typedef struct {
    osg_scene_usage usage;
    osg_scene_source source[4]; osg_render_object_info object[4];
    const osg_scene_profile *profile;
    osg_scene_properties properties[2][OSG_SCENE_MAX_NODES];
    uint16_t tile[512]; uint8_t dirty[(OSG_SCENE_MAX_TILES+7)/8];
    uint16_t nodes,variables,pages,hits,active_page,tiles_x,tiles_y,cursor;
    uint8_t front,displayed; bool admitted,failed,pending,awaiting_present,ready,connected;
    uint64_t activation_ms; uint32_t generation;
} osg_scene;
/* No allocation. Source readers remain valid and immutable until replacement.
 * Structural/content admission does not authorize source semantics or hashes. */
bool osg_scene_admit(osg_scene *,const osg_scene_source sources[4],const osg_scene_profile *);
/* Snapshot/evaluate outside transport locks. Returns false while a previous
 * slice batch is pending (without fault); caller retains/coalesces latest state.
 * Page/connect starts clock at zero; disconnect resets/freezes it at zero. */
bool osg_scene_update(osg_scene *,const osg_scene_state *);
/* At most max_tiles synchronous 64x8 writes. Call outside state/network locks.
 * Positive return is tiles written; zero means complete or failed. The caller
 * presents the completed batch before exposing it to input. */
unsigned osg_scene_render_slice(osg_scene *,unsigned max_tiles,osg_write_pixels,void *);
void osg_scene_presented(osg_scene *,bool success);
/* Returns interaction index, or -1. Input policy/ownership remains host-owned. */
int osg_scene_hit_test(osg_scene *,int x,int y);
/* Compact descriptor access for integration; read failure latches failure. */
bool osg_scene_get_usage(const osg_scene *,osg_scene_usage *);
bool osg_scene_string(osg_scene *,unsigned object,unsigned section,uint32_t offset,uint16_t bytes,char *out,size_t capacity);
bool osg_scene_record(osg_scene *,unsigned object,unsigned section,uint32_t index,void *record,size_t bytes);
#endif