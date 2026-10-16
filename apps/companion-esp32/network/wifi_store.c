#include "platform.h"
#include "esp_event.h"
#include "esp_wifi.h"
#include "esp_netif.h"
#include "esp_flash_encrypt.h"
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
bool osg_wifi_start(const osg_settings *s){
    if(!initialized){
        if(nvs_flash_init()!=ESP_OK||esp_netif_init()!=ESP_OK||esp_event_loop_create_default()!=ESP_OK)return false;
        events=xEventGroupCreate();if(!events||!esp_netif_create_default_wifi_sta())return false;
        wifi_init_config_t cfg=WIFI_INIT_CONFIG_DEFAULT();
        if(esp_wifi_init(&cfg)!=ESP_OK||esp_event_handler_register(WIFI_EVENT,ESP_EVENT_ANY_ID,changed,NULL)!=ESP_OK||
            esp_event_handler_register(IP_EVENT,IP_EVENT_STA_GOT_IP,changed,NULL)!=ESP_OK)return false;
        initialized=true;
    }
    /* Driver credentials are RAM-only even if ordinary NVS is available. */
    wifi_config_t cfg={0};memcpy(cfg.sta.ssid,s->ssid,strlen(s->ssid));memcpy(cfg.sta.password,s->password,strlen(s->password));
    cfg.sta.threshold.authmode=s->password[0]?WIFI_AUTH_WPA2_PSK:WIFI_AUTH_OPEN;
    bool ok=esp_wifi_set_storage(WIFI_STORAGE_RAM)==ESP_OK&&esp_wifi_set_mode(WIFI_MODE_STA)==ESP_OK&&esp_wifi_set_config(WIFI_IF_STA,&cfg)==ESP_OK;
    mbedtls_platform_zeroize(&cfg,sizeof(cfg));
    if(!ok||esp_wifi_start()!=ESP_OK||esp_wifi_connect()!=ESP_OK)return false;
    return (xEventGroupWaitBits(events,1,pdFALSE,pdTRUE,pdMS_TO_TICKS(15000))&1)!=0;
}
void osg_wifi_stop(void){if(initialized){esp_wifi_disconnect();esp_wifi_stop();xEventGroupClearBits(events,1);}}
bool osg_store_available(void){
#if CONFIG_NVS_ENCRYPTION
    return esp_flash_encryption_enabled();
#else
    return false;
#endif
}
bool osg_store_save(const osg_settings *s){
    if(!osg_store_available()||s->schema!=1||!osg_secret(s->token))return false;
    osg_settings stored=*s;memset(stored.secret,0,sizeof(stored.secret));stored.unix_ms=0;stored.expires_ms=0;
    nvs_handle_t nvs;if(nvs_open("osg_companion",NVS_READWRITE,&nvs)!=ESP_OK){mbedtls_platform_zeroize(&stored,sizeof(stored));return false;}
    bool ok=nvs_set_blob(nvs,"installation",&stored,sizeof(stored))==ESP_OK&&nvs_commit(nvs)==ESP_OK;
    nvs_close(nvs);mbedtls_platform_zeroize(&stored,sizeof(stored));return ok;
}
bool osg_store_load(osg_settings *s){
    if(!osg_store_available()||nvs_flash_init()!=ESP_OK)return false;
    nvs_handle_t nvs;if(nvs_open("osg_companion",NVS_READONLY,&nvs)!=ESP_OK)return false;
    size_t n=sizeof(*s);bool ok=nvs_get_blob(nvs,"installation",s,&n)==ESP_OK&&n==sizeof(*s)&&s->schema==1;
    nvs_close(nvs);
    if(!ok){mbedtls_platform_zeroize(s,sizeof(*s));return false;}
    /* Blob strings must be terminated before any credential/network use. */
    ok=memchr(s->ssid,0,sizeof(s->ssid))&&memchr(s->password,0,sizeof(s->password))&&memchr(s->endpoint,0,sizeof(s->endpoint))&&
       memchr(s->ca,0,sizeof(s->ca))&&memchr(s->pin,0,sizeof(s->pin))&&memchr(s->host,0,sizeof(s->host))&&
       memchr(s->companion,0,sizeof(s->companion))&&memchr(s->name,0,sizeof(s->name))&&memchr(s->secret,0,sizeof(s->secret))&&
       memchr(s->token,0,sizeof(s->token))&&memchr(s->instance,0,sizeof(s->instance));
    char host[254];int port;
    if(ok)ok=s->ssid[0]&&osg_secret(s->token)&&osg_stable_id(s->host)&&osg_stable_id(s->companion)&&osg_hash_id(s->pin)&&osg_endpoint(s->endpoint,host,&port)&&(!s->instance[0]||osg_stable_id(s->instance));
    if(!ok)mbedtls_platform_zeroize(s,sizeof(*s));
    return ok;
}
bool osg_store_forget(void){
    if(!osg_store_available())return true;
    if(nvs_flash_init()!=ESP_OK)return false;
    nvs_handle_t nvs;esp_err_t status=nvs_open("osg_companion",NVS_READWRITE,&nvs);if(status!=ESP_OK)return false;
    status=nvs_erase_key(nvs,"installation");bool ok=(status==ESP_OK||status==ESP_ERR_NVS_NOT_FOUND)&&nvs_commit(nvs)==ESP_OK;
    nvs_close(nvs);return ok;
}
