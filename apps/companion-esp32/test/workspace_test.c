#include "workspace.h"
#include "lvgl_pool.h"
#include "lv_conf.h"
#include "esp_heap_caps.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>

static bool fail;
static unsigned calls;
static size_t last_bytes;
static void *allocations[4];
static unsigned expected(void) { return MALLOC_CAP_8BIT|(OSG_TEST_RICH?MALLOC_CAP_SPIRAM:MALLOC_CAP_INTERNAL); }
void *heap_caps_malloc(size_t bytes,unsigned caps) {
    assert(caps==expected());last_bytes=bytes;calls++;
    if(fail)return NULL;
    for(unsigned i=0;i<4;i++)if(!allocations[i]) { allocations[i]=malloc(bytes);assert(allocations[i]);return allocations[i]; }
    assert(!"unexpected extra reservation");return NULL;
}
size_t heap_caps_get_largest_free_block(unsigned caps) { assert(caps==expected());return fail?0:65536; }
int main(void) {
    assert(osg_workspace_rich()==(bool)OSG_TEST_RICH&&osg_workspace_available()==65536);
    fail=true;assert(!osg_transport_storage_prepare()&&!osg_transport_reply&&!osg_transport_reply_capacity);
    assert(last_bytes==(OSG_TEST_RICH?192*1024:32768));assert(!osg_workspace_available());
    assert(!osg_lvgl_pool_prepare()&&!osg_lvgl_pool_allocate(LV_MEM_SIZE));
    fail=false;assert(osg_transport_storage_prepare()&&osg_transport_reply&&osg_transport_reply_capacity==(OSG_TEST_RICH?192*1024:32768));
    unsigned before=calls;assert(osg_transport_storage_prepare()&&calls==before);
    assert(osg_lvgl_pool_prepare()&&last_bytes==LV_MEM_SIZE&&osg_lvgl_pool_allocate(LV_MEM_SIZE));
    assert(osg_lvgl_pool_allocate(LV_MEM_SIZE)!=osg_transport_reply&&!osg_lvgl_pool_allocate(LV_MEM_SIZE+1));
    before=calls;assert(osg_lvgl_pool_prepare()&&calls==before);
    for(unsigned i=0;i<4;i++)free(allocations[i]);
    printf("%s workspace: allocation failure/retry, correct memory domain, bounded reply and pre-reserved LVGL pool passed\n",OSG_TEST_RICH?"PSRAM":"Internal RAM");return 0;
}
