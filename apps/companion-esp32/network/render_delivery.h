#ifndef OSG_RENDER_DELIVERY_H
#define OSG_RENDER_DELIVERY_H
#include "../render/engine.h"
typedef enum { OSG_DELIVERY_NEGOTIATE, OSG_DELIVERY_PREPARE, OSG_DELIVERY_ERASE,
    OSG_DELIVERY_READ, OSG_DELIVERY_VERIFY, OSG_DELIVERY_COMPLETE,
    OSG_DELIVERY_LEGACY, OSG_DELIVERY_ERROR } osg_delivery_phase;
typedef struct {
    osg_delivery_phase phase; char instance[65],publication[72],prepared[65],package[72],semantic[72];
    unsigned object; uint32_t offset,received,total; uint64_t keepalive_ms;
    const char *error;
} osg_render_delivery;
/* Owns request; response is caller-owned, as with osg_socket_request. Each step
 * does one request, one erase sector, or verification. Never opens/arms input. */
typedef bool (*osg_delivery_request)(void *,cJSON *,cJSON **,unsigned timeout_ms);
bool osg_delivery_start(osg_render_delivery *,const osg_engine *,const char *instance,const char *publication);
osg_delivery_phase osg_delivery_step(osg_render_delivery *,osg_engine *,osg_delivery_request,void *,uint64_t now_ms,bool main_suspended);
cJSON *osg_delivery_open_request(const osg_render_delivery *,const osg_engine *);
#endif
