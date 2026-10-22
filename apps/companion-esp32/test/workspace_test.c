#include "workspace.h"
#include "lvgl_pool.h"
#include "lv_conf.h"
#include "render_profile.h"
#include "esp_heap_caps.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <stdint.h>
#include <string.h>

static bool fail;
static unsigned calls;
static size_t last_bytes;
static size_t last_alignment;
static void *allocations[4];
static unsigned expected(void) { return MALLOC_CAP_8BIT|(OSG_TEST_RICH?MALLOC_CAP_SPIRAM:MALLOC_CAP_INTERNAL); }
static size_t json_calls,json_live,json_fail=SIZE_MAX;
static void *profile_allocate(size_t bytes){
    if(json_calls++==json_fail)return NULL;
    unsigned char *p=malloc(sizeof(max_align_t)+bytes);assert(p);json_live++;return p+sizeof(max_align_t);
}
static void profile_release(void *pointer){if(pointer){assert(json_live);json_live--;free((unsigned char *)pointer-sizeof(max_align_t));}}
static void profile_failures(void){
    cJSON_Hooks hooks={profile_allocate,profile_release};cJSON_InitHooks(&hooks);
    json_calls=0;cJSON *profile=osg_render_profile_json(800,480);assert(profile);size_t calls=json_calls;cJSON_Delete(profile);assert(!json_live);
    for(size_t failure=0;failure<calls;failure++){
        json_calls=0;json_fail=failure;profile=osg_render_profile_json(800,480);assert(!profile&&!json_live);
    }
    cJSON_InitHooks(NULL);printf("Renderer report allocation sites: %u failures leave no partial report or live allocations\n",(unsigned)calls);
}
void *heap_caps_aligned_alloc(size_t alignment,size_t bytes,unsigned caps) {
    assert(alignment==_Alignof(max_align_t));
    last_alignment=alignment;
    assert(caps==expected());last_bytes=bytes;calls++;
    if(fail)return NULL;
    for(unsigned i=0;i<4;i++)if(!allocations[i]) { allocations[i]=malloc(bytes);assert(allocations[i]);return allocations[i]; }
    assert(!"unexpected extra reservation");return NULL;
}
size_t heap_caps_get_largest_free_block(unsigned caps) { assert(caps==expected());return fail?0:65536; }
void *heap_caps_malloc(size_t bytes,unsigned caps) { void *p=heap_caps_aligned_alloc(_Alignof(max_align_t),bytes,caps);last_alignment=0;return p; }
static void profile_check(const char *path){
    assert(!osg_render_profile_json(63,480)&&!osg_render_profile_json(800,1025));
    cJSON *profile=osg_render_profile_json(800,480);assert(profile);
    const osg_render_limits *limits=osg_render_profile_limits();
    assert(cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(cJSON_GetObjectItemCaseSensitive(profile,"limits"),"sceneBytes"))==limits->scene_bytes);
    char *encoded=cJSON_PrintUnformatted(profile);assert(encoded&&strlen(encoded)<2048);FILE *f=fopen(path,"wb");assert(f&&fputs(encoded,f)>=0);fclose(f);cJSON_free(encoded);cJSON_Delete(profile);
}
int main(int argc,char **argv) {
    assert(argc==2);profile_check(argv[1]);profile_failures();
    assert(osg_workspace_rich()==(bool)OSG_TEST_RICH&&osg_workspace_available()==65536);
    fail=true;assert(!osg_transport_storage_prepare()&&!osg_transport_reply&&!osg_transport_reply_capacity);
    assert(last_bytes==(OSG_TEST_RICH?192*1024:32768));assert(!osg_workspace_available());
    assert(!osg_lvgl_pool_prepare()&&!osg_lvgl_pool_allocate(LV_MEM_SIZE));
    fail=false;assert(osg_transport_storage_prepare()&&osg_transport_reply&&osg_transport_reply_capacity==(OSG_TEST_RICH?192*1024:32768));
    assert(last_alignment==_Alignof(max_align_t));
    assert((uintptr_t)osg_transport_reply%_Alignof(max_align_t)==0);
    unsigned before=calls;assert(osg_transport_storage_prepare()&&calls==before);
    assert(osg_lvgl_pool_prepare()&&last_bytes==LV_MEM_SIZE&&osg_lvgl_pool_allocate(LV_MEM_SIZE));
    assert(osg_lvgl_pool_allocate(LV_MEM_SIZE)!=osg_transport_reply&&!osg_lvgl_pool_allocate(LV_MEM_SIZE+1));
    before=calls;assert(osg_lvgl_pool_prepare()&&calls==before);
    for(unsigned i=0;i<4;i++)free(allocations[i]);
    printf("%s workspace: allocation failure/retry, correct memory domain, bounded reply and pre-reserved LVGL pool passed\n",OSG_TEST_RICH?"PSRAM":"Internal RAM");return 0;
}
