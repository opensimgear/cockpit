#include "lvgl_pool.h"
#include "lv_conf.h"
#if defined(ESP_PLATFORM) && defined(CONFIG_SPIRAM) && CONFIG_SPIRAM
#include "esp_heap_caps.h"
static void *pool;
bool osg_lvgl_pool_prepare(void) {
    if(!pool)pool=heap_caps_malloc(LV_MEM_SIZE,MALLOC_CAP_SPIRAM|MALLOC_CAP_8BIT);
    return pool!=NULL;
}
void *osg_lvgl_pool_allocate(size_t bytes) { return bytes==LV_MEM_SIZE?pool:NULL; }
#else
bool osg_lvgl_pool_prepare(void) { return true; }
void *osg_lvgl_pool_allocate(size_t bytes) { (void)bytes;return NULL; }
#endif
