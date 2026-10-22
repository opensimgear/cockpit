#include "render_profile.h"
#include "workspace.h"
#include "firmware.h"
#include "companion.h"
#include "widget.h"
#include "render_profile_data.h"
_Static_assert(OSG_MAX_WIDGETS==OSG_RENDER_COMPACT_WIDGETS&&OSG_MAX_WIDGETS==OSG_RENDER_RICH_WIDGETS&&
    OSG_MAX_PAGES==OSG_RENDER_COMPACT_PAGES&&OSG_MAX_PAGES==OSG_RENDER_RICH_PAGES&&
    OSG_WIDGET_SOURCE_PORTS==OSG_RENDER_COMPACT_PORTS_PER_WIDGET&&OSG_WIDGET_SOURCE_PORTS==OSG_RENDER_RICH_PORTS_PER_WIDGET,
    "Update the shared render profile for changed engine bounds");
const osg_render_limits *osg_render_profile_limits(void){return osg_workspace_rich()?&rich_limits:&compact_limits;}
cJSON *osg_render_profile_json(uint16_t width,uint16_t height){
    if(width<64||height<64||width>1024||height>1024)return NULL;
    cJSON *profile=cJSON_Parse(osg_workspace_rich()?rich_profile:compact_profile);
    if(!profile)return NULL;
    cJSON *screen=cJSON_AddObjectToObject(profile,"screen");
    if(!screen||!cJSON_AddNumberToObject(screen,"width",width)||!cJSON_AddNumberToObject(screen,"height",height)||
       !cJSON_AddStringToObject(profile,"firmwareVersion",OSG_FIRMWARE_VERSION)||!cJSON_AddStringToObject(profile,"boardProfile",OSG_BOARD_PROFILE)){
        cJSON_Delete(profile);return NULL;
    }
    return profile;
}
