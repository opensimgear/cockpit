#ifndef OSG_RENDER_PROFILE_H
#define OSG_RENDER_PROFILE_H
#include "render_profile_limits.h"
#include "cJSON.h"
#include <stdint.h>
const osg_render_limits *osg_render_profile_limits(void);
/* Network-task metadata only; this never changes input or session authority. */
cJSON *osg_render_profile_json(uint16_t width,uint16_t height);
#endif
