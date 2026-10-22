#include "lvgl_pool.h"
#include "lv_conf.h"
#ifdef ESP_PLATFORM
#include "esp_heap_caps.h"
static void *pool;
bool osg_lvgl_pool_prepare(void) {
    unsigned caps=MALLOC_CAP_8BIT;
#if defined(CONFIG_SPIRAM) && CONFIG_SPIRAM
    caps|=MALLOC_CAP_SPIRAM;
#else
    caps|=MALLOC_CAP_INTERNAL;
#endif
    if(!pool)pool=heap_caps_malloc(LV_MEM_SIZE,caps);
    return pool!=NULL;
}
void *osg_lvgl_pool_allocate(size_t bytes) { return bytes==LV_MEM_SIZE?pool:NULL; }
#else
bool osg_lvgl_pool_prepare(void) { return true; }
void *osg_lvgl_pool_allocate(size_t bytes) { (void)bytes;return NULL; }
#endif
