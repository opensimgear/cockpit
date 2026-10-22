#ifndef OSG_LVGL_POOL_H
#define OSG_LVGL_POOL_H
#include <stdbool.h>
#include <stddef.h>
/* Reserve the rich-board pool before lv_init(), whose builtin allocator
 * assumes a valid pool. The pool belongs to the display-task process lifetime. */
bool osg_lvgl_pool_prepare(void);
void *osg_lvgl_pool_allocate(size_t bytes);
#endif
