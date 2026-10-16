#include "platform.h"
#include "nvs_flash.h"
#include "nvs.h"
#include "mbedtls/platform_util.h"
#include <string.h>
#include <stddef.h>
/* NVS commits survive power loss. Never erase the partition on init failure.
 * Encryption, when configured, is supplied by ESP-IDF NVS.
 * Ordinary boards use unencrypted NVS; this code never changes eFuses. */
bool osg_store_available(void){return nvs_flash_init()==ESP_OK;}
bool osg_store_save(const osg_settings *s){
    if(!osg_store_available()||s->schema!=2||!osg_secret(s->token))return false;
    osg_settings stored=*s;memset(stored.secret,0,sizeof(stored.secret));stored.unix_ms=0;stored.expires_ms=0;
    /* Assignment is owned by Cockpit and resolved on every connection. */
    memset(stored.instance,0,sizeof(stored.instance));
    nvs_handle_t nvs;if(nvs_open("osg_companion",NVS_READWRITE,&nvs)!=ESP_OK){mbedtls_platform_zeroize(&stored,sizeof(stored));return false;}
    bool ok=nvs_set_blob(nvs,"installation",&stored,sizeof(stored))==ESP_OK&&nvs_commit(nvs)==ESP_OK;
    nvs_close(nvs);mbedtls_platform_zeroize(&stored,sizeof(stored));return ok;
}
bool osg_store_load(osg_settings *s){
    if(!osg_store_available()||nvs_flash_init()!=ESP_OK)return false;
    nvs_handle_t nvs;if(nvs_open("osg_companion",NVS_READONLY,&nvs)!=ESP_OK)return false;
    size_t n=sizeof(*s);bool ok=nvs_get_blob(nvs,"installation",s,&n)==ESP_OK&&n==sizeof(*s)&&s->schema==2;
    nvs_close(nvs);
    /* Validate on-flash boolean bytes before interpreting their C representation. */
    const unsigned char *raw=(const unsigned char *)s;
    if(ok)ok=raw[offsetof(osg_settings,persist)]<=1&&raw[offsetof(osg_settings,direct)]<=1;
    if(!ok){mbedtls_platform_zeroize(s,sizeof(*s));return false;}
    /* Blob strings must be terminated before any credential/network use. */
    ok=memchr(s->ssid,0,sizeof(s->ssid))&&memchr(s->password,0,sizeof(s->password))&&memchr(s->endpoint,0,sizeof(s->endpoint))&&
       memchr(s->ca,0,sizeof(s->ca))&&memchr(s->pin,0,sizeof(s->pin))&&memchr(s->host,0,sizeof(s->host))&&
       memchr(s->companion,0,sizeof(s->companion))&&memchr(s->name,0,sizeof(s->name))&&memchr(s->secret,0,sizeof(s->secret))&&
       memchr(s->token,0,sizeof(s->token))&&memchr(s->instance,0,sizeof(s->instance));
    char host[254];int port;
    if(ok)ok=s->persist&&osg_secret(s->token)&&osg_stable_id(s->host)&&osg_stable_id(s->companion)&&s->name[0]&&(!s->instance[0]||osg_stable_id(s->instance))&&(s->direct||(s->ssid[0]&&s->ca[0]&&osg_hash_id(s->pin)&&osg_endpoint(s->endpoint,host,&port)));
    if(!ok)mbedtls_platform_zeroize(s,sizeof(*s));
    return ok;
}
bool osg_store_forget(void){
    if(!osg_store_available())return false;
    if(nvs_flash_init()!=ESP_OK)return false;
    nvs_handle_t nvs;esp_err_t status=nvs_open("osg_companion",NVS_READWRITE,&nvs);if(status!=ESP_OK)return false;
    status=nvs_erase_key(nvs,"installation");bool ok=(status==ESP_OK||status==ESP_ERR_NVS_NOT_FOUND)&&nvs_commit(nvs)==ESP_OK;
    nvs_close(nvs);return ok;
}
