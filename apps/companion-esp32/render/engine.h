#ifndef OSG_RENDER_ENGINE_H
#define OSG_RENDER_ENGINE_H
#include "content_store.h"
#include "scene.h"
#include "../network/wire.h"
#define OSG_ENGINE_NONE 65535u
#define OSG_ENGINE_HITS 32u
#ifdef OSG_RENDER_SUNTON
#define OSG_ENGINE_MANIFEST_BYTES 4096u
#else
#define OSG_ENGINE_MANIFEST_BYTES 8192u
#endif
/* Admission includes measured scene temporary work and a bounded allowance for
 * delivery state, the immutable touch cache and partition SHA context. The
 * platform must statically check those auxiliary instances fit this reserve. */
#define OSG_ENGINE_SCENE_WORK_BYTES 2048u
#define OSG_ENGINE_AUXILIARY_BYTES 1536u

typedef struct {uint16_t interaction,page,widget,destination_page;int16_t x,y;uint16_t width,height;} osg_engine_hit;
typedef struct {osg_engine_hit hits[OSG_ENGINE_HITS];uint16_t count,page;uint32_t generation;bool ready;} osg_engine_hit_cache;
typedef struct {
    osg_content_store store;osg_scene scene;osg_content_view active_view,validation_view;osg_content_reader readers[4];
    const osg_scene_profile *profile;const char *profile_id,*profile_hash;
    char package_hash[72],source_hash[72],semantic_hash[72],ui_id[65];
    char manifest[OSG_ENGINE_MANIFEST_BYTES+1];size_t manifest_bytes;
    uint16_t control_widgets[32],output_widgets[64],variable_outputs[64],interaction_widgets[32];
    osg_content_candidate pending;osg_content_status status;bool initialized,available,active,staging,ready,failed;
} osg_engine;
/* One engine/scene per firmware. Call only on the network task after JSON-hook
 * initialization. Boot scan restores validated offline content, never input. */
osg_content_status osg_engine_initialize(osg_engine *,const char *board_profile,uint16_t width,uint16_t height);
/* Native/fault-injection boundary: supplies raw flash/SHA callbacks; engine owns
 * manifest/content validators and binds them to this instance. */
osg_content_status osg_engine_initialize_io(osg_engine *,const char *board_profile,uint16_t width,uint16_t height,osg_content_io);
const char *osg_engine_profile_id(const osg_engine *);
const char *osg_engine_profile_hash(const osg_engine *);
const char *osg_engine_current_package(const osg_engine *);
const osg_content_candidate *osg_engine_metadata(const osg_engine *);
/* Root must suspend scene rendering before begin/finish/abort; main_suspended
 * is an explicit assertion of that handshake. Input stays stopped throughout.
 * This module reuses its sole scene for verification and restores old content
 * on failure. A chunk never owns a caller buffer beyond its synchronous call. */
osg_content_status osg_engine_begin(osg_engine *,const char *manifest_utf8,size_t bytes,const char *package_hash,bool main_suspended);
osg_content_status osg_engine_erase_step(osg_engine *,bool *done);
osg_content_status osg_engine_write(osg_engine *,unsigned object,uint32_t offset,const void *,size_t);
osg_content_status osg_engine_finish(osg_engine *,bool main_suspended);
osg_content_status osg_engine_abort(osg_engine *,bool main_suspended);
/* Builds only the legacy logical/session shim. Drawing always uses scene.c.
 * The caller provides an inactive osg_image slot and publishes it atomically. */
bool osg_engine_make_panel(osg_engine *,osg_image *);
/* Called outside network locks on a stable runtime snapshot. Coalesce while a
 * previous slice batch is pending; false without failed means try again later. */
bool osg_engine_update_from_runtime(osg_engine *,const osg_runtime *,uint64_t now_ms);
unsigned osg_engine_render_slice(osg_engine *,unsigned max_tiles,osg_write_pixels,void *);
void osg_engine_presented(osg_engine *,bool success);
/* Main copies after successful presentation; root publishes this immutable
 * value under ui.mutex. Touch must never read/mutate the live scene directly. */
bool osg_engine_copy_hits(osg_engine *,osg_engine_hit_cache *);
#endif
