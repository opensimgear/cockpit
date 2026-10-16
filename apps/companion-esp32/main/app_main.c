#include "companion.h"
#include "raster.h"
#include "board.h"
#include "panel.h"
#include "platform.h"
#include "diagnostics.h"
#include "esp_heap_caps.h"
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
    int64_t previous=0,last_stack=0;
    for(;;){bool down=false;int x=0,y=0;const int64_t started=esp_timer_get_time();
        if(previous)osg_diagnostics_record(OSG_DIAG_TOUCH_INTERVAL,(uint64_t)(started-previous),0,true);
        previous=started;
        bool valid=board->touch(board->context,&down,&x,&y);
        osg_diagnostics_record(OSG_DIAG_TOUCH_POLL,(uint64_t)(esp_timer_get_time()-started),0,valid);
        if(started-last_stack>=1000000){osg_diagnostics_stack(OSG_DIAG_TOUCH_STACK,(uint32_t)uxTaskGetStackHighWaterMark(NULL));last_stack=started;}
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
    uint64_t last_render=0,last_resources=0;
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
            int64_t started=esp_timer_get_time();
            osg_raster_render(&raster,&drawing,damage,now);last_render=now;
            osg_diagnostics_record(OSG_DIAG_RENDER,(uint64_t)(esp_timer_get_time()-started),0,!raster.failed);
            if(!raster.failed&&board.present){
                started=esp_timer_get_time();const bool presented=board.present(board.context);
                osg_diagnostics_record(OSG_DIAG_PRESENT,(uint64_t)(esp_timer_get_time()-started),0,presented);
                if(!presented)raster.failed=true;
            }
            retry_full=raster.failed;
            if(!raster.failed)memcpy(last_hash,drawing.panel->publication_hash,sizeof(last_hash));
            osg_network_render(drawing.panel,true,!raster.failed);
            xSemaphoreTake(mutex,portMAX_DELAY);drawing_panel=NULL;xSemaphoreGive(mutex);
            if(raster.failed){ESP_LOGE("companion","Display flush failed; live input cancelled");raster.failed=false;}
        }
        if(now-last_resources>=1000){
            osg_diagnostics_memory((uint32_t)heap_caps_get_free_size(MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT),
                (uint32_t)heap_caps_get_minimum_free_size(MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT),
                (uint32_t)heap_caps_get_largest_free_block(MALLOC_CAP_INTERNAL|MALLOC_CAP_8BIT));
            /* ESP-IDF reports stack high-water marks in bytes. */
            osg_diagnostics_stack(OSG_DIAG_MAIN_STACK,(uint32_t)uxTaskGetStackHighWaterMark(NULL));last_resources=now;
        }
        vTaskDelay(pdMS_TO_TICKS(20));
    }
}
