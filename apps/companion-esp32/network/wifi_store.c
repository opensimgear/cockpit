#include "platform.h"
#include "esp_event.h"
#include "esp_wifi.h"
#include "esp_netif.h"
#include "esp_netif_sntp.h"
#include <time.h>

#include "nvs_flash.h"
#include "nvs.h"
#include "freertos/event_groups.h"
#include "mbedtls/platform_util.h"
#include <string.h>
static EventGroupHandle_t events;
static bool initialized;
static void changed(void *arg,esp_event_base_t base,int32_t id,void *data){
    (void)arg;(void)data;
    if(base==IP_EVENT&&id==IP_EVENT_STA_GOT_IP)xEventGroupSetBits(events,1);
    if(base==WIFI_EVENT&&id==WIFI_EVENT_STA_DISCONNECTED)xEventGroupClearBits(events,1);
}
static bool wifi_init(void){
    if(!initialized){
        if(nvs_flash_init()!=ESP_OK||esp_netif_init()!=ESP_OK||esp_event_loop_create_default()!=ESP_OK)return false;
        events=xEventGroupCreate();if(!events||!esp_netif_create_default_wifi_sta())return false;
        wifi_init_config_t cfg=WIFI_INIT_CONFIG_DEFAULT();
        if(esp_wifi_init(&cfg)!=ESP_OK||esp_event_handler_register(WIFI_EVENT,ESP_EVENT_ANY_ID,changed,NULL)!=ESP_OK||
            esp_event_handler_register(IP_EVENT,IP_EVENT_STA_GOT_IP,changed,NULL)!=ESP_OK)return false;
        initialized=true;
    }
    return true;
}
bool osg_wifi_start(const osg_settings *s){
    if(!wifi_init())return false;
    /* Driver credentials are RAM-only even if ordinary NVS is available. */
    wifi_config_t cfg={0};memcpy(cfg.sta.ssid,s->ssid,strlen(s->ssid));memcpy(cfg.sta.password,s->password,strlen(s->password));
    cfg.sta.threshold.authmode=s->password[0]?WIFI_AUTH_WPA2_PSK:WIFI_AUTH_OPEN;
    bool ok=esp_wifi_set_storage(WIFI_STORAGE_RAM)==ESP_OK&&esp_wifi_set_mode(WIFI_MODE_STA)==ESP_OK&&esp_wifi_set_config(WIFI_IF_STA,&cfg)==ESP_OK;
    mbedtls_platform_zeroize(&cfg,sizeof(cfg));
    if(!ok||esp_wifi_start()!=ESP_OK||esp_wifi_connect()!=ESP_OK)return false;
    return (xEventGroupWaitBits(events,1,pdFALSE,pdTRUE,pdMS_TO_TICKS(15000))&1)!=0;
}
void osg_wifi_stop(void){if(initialized){esp_wifi_disconnect();esp_wifi_stop();xEventGroupClearBits(events,1);}}
/* Time bootstrap only: TLS still validates CA, hostname, dates and the pinned
 * Cockpit identity. Never persist a stale wall clock across power loss. */
bool osg_wifi_clock(void){
    esp_sntp_config_t config=ESP_NETIF_SNTP_DEFAULT_CONFIG("pool.ntp.org");
    if(esp_netif_sntp_init(&config)!=ESP_OK)return false;
    bool ok=esp_netif_sntp_sync_wait(pdMS_TO_TICKS(15000))==ESP_OK;
    esp_netif_sntp_deinit();
    time_t now=time(NULL);
    return ok&&now>=1735689600&&now<=4102444800LL;
}
cJSON *osg_wifi_scan(void){
    if(!wifi_init()||esp_wifi_set_storage(WIFI_STORAGE_RAM)!=ESP_OK||
       esp_wifi_set_mode(WIFI_MODE_STA)!=ESP_OK||esp_wifi_start()!=ESP_OK)return NULL;
    wifi_scan_config_t scan={0};scan.show_hidden=false;scan.scan_time.active.min=40;scan.scan_time.active.max=120;
    if(esp_wifi_scan_start(&scan,true)!=ESP_OK)return NULL;
    wifi_ap_record_t records[24];uint16_t count=24;
    if(esp_wifi_scan_get_ap_records(&count,records)!=ESP_OK)return NULL;
    cJSON *list=cJSON_CreateArray();
    for(unsigned i=0;i<count;i++){
        records[i].ssid[32]=0;if(!records[i].ssid[0])continue;
        cJSON *row=cJSON_CreateObject();cJSON_AddStringToObject(row,"ssid",(char *)records[i].ssid);
        cJSON_AddNumberToObject(row,"rssi",records[i].rssi);
        cJSON_AddBoolToObject(row,"secured",records[i].authmode!=WIFI_AUTH_OPEN);cJSON_AddItemToArray(list,row);
    }
    esp_wifi_stop();return list;
}
