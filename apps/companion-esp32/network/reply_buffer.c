#include "workspace.h"
#include "wire.h"
#include "render_profile.h"
char *osg_transport_reply;
size_t osg_transport_reply_capacity;
bool osg_transport_storage_prepare(void) {
    if(osg_transport_reply)return true;
    const size_t bytes=osg_render_profile_limits()->reply_bytes;
    osg_transport_reply=osg_workspace_allocate(bytes);
    if(!osg_transport_reply)return false;
    osg_transport_reply_capacity=bytes;return true;
}
