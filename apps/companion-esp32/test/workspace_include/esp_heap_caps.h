#ifndef OSG_TEST_HEAP_CAPS_H
#define OSG_TEST_HEAP_CAPS_H
#include <stddef.h>
#define MALLOC_CAP_8BIT 1
#define MALLOC_CAP_INTERNAL 2
#define MALLOC_CAP_SPIRAM 4
void *heap_caps_malloc(size_t,unsigned);
size_t heap_caps_get_largest_free_block(unsigned);
#endif
