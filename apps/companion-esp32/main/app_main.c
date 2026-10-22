#include "companion.h"
#include "panel_draw.h"
#include "board.h"
#include "panel.h"
#include "platform.h"
#include "candidate.h"
#include "render_profile.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_idf_version.h"
#include "esp_heap_caps.h"
#include <string.h>
#include <stdlib.h>
#if ESP_IDF_VERSION != ESP_IDF_VERSION_VAL(5, 5, 2)
#error "Use the pinned ESP-IDF v5.5.2 toolchain"
#endif
static osg_runtime runtime, drawing;
static osg_panel_draw renderer;
static bool dirty=true;
static const osg_panel *drawing_panel;
static void touch_task(void *context){
    osg_board *board=context;
    for(;;){bool down=false;int x=0,y=0;bool valid=board->touch(board->context,&down,&x,&y);
        osg_network_touch(valid,down,x,y);vTaskDelay(pdMS_TO_TICKS(20));}
}
void app_main(void) {
    osg_board board;
    if (!osg_board_init(&board) || !board.write) { ESP_LOGE("companion", "Board initialization failed"); return; }
    const size_t bytes=(size_t)board.width*OSG_WIDGET_DRAW_ROWS*sizeof(uint16_t);
    unsigned caps=MALLOC_CAP_8BIT;
#if defined(CONFIG_SPIRAM) && CONFIG_SPIRAM
    caps|=MALLOC_CAP_SPIRAM;
#else
    caps|=MALLOC_CAP_INTERNAL;
#endif
    void *buffer=heap_caps_aligned_alloc(LV_DRAW_BUF_ALIGN,bytes,caps);
    if(!buffer||!osg_panel_draw_init(&renderer,board,buffer,bytes)) {
        ESP_LOGE("companion","LVGL display pool/buffer allocation failed (%u buffer bytes)",(unsigned)bytes);free(buffer);return;
    }
    renderer.max_font_size=(unsigned)osg_render_profile_limits()->font_size;
    if (!osg_init(&runtime, &osg_demo_panel, osg_raster_display(&renderer.diagnostic_font, board.width, board.height), NULL, NULL)) {
        ESP_LOGE("companion", "Unsupported panel/display profile"); return;
    }
    SemaphoreHandle_t mutex=xSemaphoreCreateMutex();if(!mutex)return;
    osg_network_start((osg_network_ui){&runtime,mutex,&dirty,&drawing_panel,board.touch!=NULL});
    if(board.touch&&xTaskCreate(touch_task,"companion-touch",8192,&board,5,NULL)!=pdPASS)
        ESP_LOGE("companion","Touch task allocation failed; live arming stays unavailable");
    uint64_t last_render=0;
    char last_hash[72]={0};bool retry_full=false;
    for (;;) {
        uint64_t now=(uint64_t)(esp_timer_get_time()/1000);
        osg_scene_ticket ticket;const osg_panel *candidate=osg_network_candidate_begin(&ticket);
        if(candidate){
            xSemaphoreTake(mutex,portMAX_DELAY);osg_display display=runtime.display;xSemaphoreGive(mutex);
            osg_panel_source_diagnostic diagnostic={0};
            bool accepted=osg_init(&drawing,candidate,display,NULL,NULL);
            if(!accepted){diagnostic.code=OSG_PANEL_SOURCE_INVALID;memcpy(diagnostic.field,"runtime.layout",sizeof("runtime.layout"));}
            else if(!(accepted=osg_panel_draw_check(&renderer,&drawing,now,true))){
                diagnostic.code=renderer.diagnostic.code==OSG_WIDGET_DRAW_BUDGET?OSG_PANEL_SOURCE_BUDGET:OSG_PANEL_SOURCE_UNSUPPORTED;
                memcpy(diagnostic.widget,renderer.diagnostic.widget,sizeof(diagnostic.widget));memcpy(diagnostic.layer,renderer.diagnostic.layer,sizeof(diagnostic.layer));
                memcpy(diagnostic.field,renderer.diagnostic.field,sizeof(diagnostic.field));diagnostic.required=renderer.diagnostic.required;diagnostic.available=renderer.diagnostic.available;
            }
            osg_network_candidate_finish(ticket,accepted,&diagnostic);
        }
        xSemaphoreTake(mutex,portMAX_DELAY);
        osg_damage damage=osg_render_damage(&runtime,&drawing,now,last_render);
        if(retry_full||strcmp(last_hash,runtime.panel->publication_hash))damage.full=true;
        const bool render=dirty||damage.full||damage.widgets;
        if(render){
            drawing=runtime;drawing_panel=drawing.panel;dirty=false;
        }
        xSemaphoreGive(mutex);
        /* Driver waits never hold the network state mutex. An image slot in use
         * here cannot be overwritten by the download worker. */
        if(render){
            osg_network_render(drawing.panel,false,true);
            const bool success=osg_panel_draw_render(&renderer,&drawing,damage,now);
            retry_full=!success;
            if(success){last_render=now;memcpy(last_hash,drawing.panel->publication_hash,sizeof(last_hash));}
            osg_network_render(drawing.panel,true,success);
            xSemaphoreTake(mutex,portMAX_DELAY);drawing_panel=NULL;xSemaphoreGive(mutex);
            if(!success)ESP_LOGE("companion","Display rejected/failed: widget=%s layer=%s field=%s required=%u available=%u; live input cancelled",renderer.diagnostic.widget,renderer.diagnostic.layer,renderer.diagnostic.field,(unsigned)renderer.diagnostic.required,(unsigned)renderer.diagnostic.available);
        }
        vTaskDelay(pdMS_TO_TICKS(20));
    }
}
