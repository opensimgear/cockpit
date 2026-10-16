#include "companion.h"
#include "raster.h"
#include "board.h"
#include "panel.h"
#include "platform.h"
#include "esp_log.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/task.h"
#include "esp_idf_version.h"
#include <string.h>
#if ESP_IDF_VERSION != ESP_IDF_VERSION_VAL(5, 5, 2)
#error "Use the pinned ESP-IDF v5.5.2 toolchain"
#endif
static osg_runtime runtime, drawing;
static osg_raster raster;
static bool dirty=true;
static const osg_panel *drawing_panel;
static bool stale(const osg_sample *s,uint64_t now){return s->available&&(now<s->received_ms||s->source_age_ms>=OSG_STALE_MS||now-s->received_ms>=OSG_STALE_MS-s->source_age_ms);}
static void touch_task(void *context){
    osg_board *board=context;
    for(;;){bool down=false;int x=0,y=0;bool valid=board->touch(board->context,&down,&x,&y);
        osg_network_touch(valid,down,x,y);vTaskDelay(pdMS_TO_TICKS(20));}
}
void app_main(void) {
    osg_board board;
    if (!osg_board_init(&board) || !board.write) { ESP_LOGE("companion", "Board initialization failed"); return; }
    raster.context = board.context; raster.write = board.write;
    if (!osg_init(&runtime, &osg_demo_panel, osg_raster_display(&raster, board.width, board.height), NULL, NULL)) {
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
        xSemaphoreTake(mutex,portMAX_DELAY);
        for(size_t i=0;i<runtime.panel->widget_count;i++)if(stale(&runtime.samples[i],now)!=stale(&runtime.samples[i],last_render))dirty=true;
        const bool render=dirty;
        osg_damage damage={0};
        if(render){
            damage=osg_render_damage(&runtime,&drawing,now,last_render);
            if(retry_full || strcmp(last_hash,runtime.panel->publication_hash))damage.full=true;
            drawing=runtime;drawing_panel=drawing.panel;dirty=false;
        }
        xSemaphoreGive(mutex);
        /* Driver waits never hold the network state mutex. An image slot in use
         * here cannot be overwritten by the download worker. */
        if(render){
            osg_network_render(drawing.panel,false,true);
            osg_raster_render(&raster,&drawing,damage,now);last_render=now;
            if(!raster.failed&&board.present&&!board.present(board.context))raster.failed=true;
            retry_full=raster.failed;
            if(!raster.failed)memcpy(last_hash,drawing.panel->publication_hash,sizeof(last_hash));
            osg_network_render(drawing.panel,true,!raster.failed);
            xSemaphoreTake(mutex,portMAX_DELAY);drawing_panel=NULL;xSemaphoreGive(mutex);
            if(raster.failed){ESP_LOGE("companion","Display flush failed; live input cancelled");raster.failed=false;}
        }
        vTaskDelay(pdMS_TO_TICKS(20));
    }
}
