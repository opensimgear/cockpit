#ifndef OSG_WORKSPACE_H
#define OSG_WORKSPACE_H
#include <stdbool.h>
#include <stddef.h>
/* Large transport and JSON storage uses the same board memory domain as future
 * immutable scene banks. Native tests use the ordinary allocator. */
bool osg_workspace_rich(void);
void *osg_workspace_allocate(size_t bytes);
size_t osg_workspace_available(void);
extern char *osg_transport_reply;
extern size_t osg_transport_reply_capacity;
bool osg_transport_storage_prepare(void);
#endif
