#include "workspace.h"
#include <stdint.h>
#include <stdlib.h>
#ifdef ESP_PLATFORM
#include "sdkconfig.h"
#include "esp_heap_caps.h"
static unsigned capabilities(void) {
    return MALLOC_CAP_8BIT|(osg_workspace_rich()?MALLOC_CAP_SPIRAM:MALLOC_CAP_INTERNAL);
}
#endif
bool osg_workspace_rich(void) {
#if defined(ESP_PLATFORM) && defined(CONFIG_SPIRAM) && CONFIG_SPIRAM
    return true;
#else
    return false;
#endif
}
void *osg_workspace_allocate(size_t bytes) {
#ifdef ESP_PLATFORM
    return heap_caps_malloc(bytes,capabilities());
#else
    return malloc(bytes);
#endif
}
size_t osg_workspace_available(void) {
#ifdef ESP_PLATFORM
    return heap_caps_get_largest_free_block(capabilities());
#else
    return 0; /* Native malloc exposes no portable largest-free-block query. */
#endif
}
