#include "../network/storage.c"
#include <assert.h>
#include <stdio.h>
void mbedtls_platform_zeroize(void *p,size_t n){volatile unsigned char *b=p;while(n--)*b++=0;}
static osg_settings disk;
static bool exists;
static int init_error,write_error,commit_error;
esp_err_t nvs_flash_init(void){return init_error;}
esp_err_t nvs_open(const char *name,int mode,nvs_handle_t *h){assert(!strcmp(name,"osg_companion"));(void)mode;*h=1;return 0;}
esp_err_t nvs_set_blob(nvs_handle_t h,const char *key,const void *p,size_t n){(void)h;assert(!strcmp(key,"installation")&&n==sizeof(disk));if(write_error)return write_error;memcpy(&disk,p,n);exists=true;return 0;}
esp_err_t nvs_get_blob(nvs_handle_t h,const char *key,void *p,size_t *n){(void)h;(void)key;if(!exists)return ESP_ERR_NVS_NOT_FOUND;assert(*n==sizeof(disk));memcpy(p,&disk,*n);return 0;}
esp_err_t nvs_commit(nvs_handle_t h){(void)h;return commit_error;}
esp_err_t nvs_erase_key(nvs_handle_t h,const char *key){(void)h;(void)key;bool old=exists;exists=false;return old?0:ESP_ERR_NVS_NOT_FOUND;}
void nvs_close(nvs_handle_t h){(void)h;}
int main(void){
 osg_settings original={.schema=2,.persist=true,.direct=true}, restored;
 strcpy(original.token,"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");
 strcpy(original.host,"host");strcpy(original.companion,"screen");strcpy(original.name,"Screen");
 strcpy(original.password,"private-password");strcpy(original.secret,"temporary-offer");strcpy(original.instance,"old-panel");
 original.unix_ms=1800000000000ULL;
 assert(osg_store_save(&original));
 memset(&restored,0,sizeof(restored));assert(osg_store_load(&restored));
 assert(restored.direct&&restored.persist&&!strcmp(restored.password,original.password)&&!strcmp(restored.token,original.token));
 assert(!restored.secret[0]&&!restored.instance[0]&&!restored.unix_ms&&!restored.expires_ms);
 assert(!strcmp(original.instance,"old-panel")&&!strcmp(original.secret,"temporary-offer"));
 disk.schema=1;assert(!osg_store_load(&restored));assert(!restored.token[0]);disk.schema=2;
 memset(disk.password,'x',sizeof(disk.password));assert(!osg_store_load(&restored));
 assert(osg_store_save(&original));disk.direct=false;assert(!osg_store_load(&restored));
 assert(osg_store_save(&original));((unsigned char *)&disk)[offsetof(osg_settings,direct)]=255;assert(!osg_store_load(&restored));
 original.direct=false;strcpy(original.ssid,"wifi");strcpy(original.ca,"certificate");strcpy(original.endpoint,"wss://host:54437/companion");
 strcpy(original.pin,"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
 assert(osg_store_save(&original));assert(osg_store_load(&restored));
 assert(!restored.direct&&!strcmp(restored.ssid,"wifi")&&!restored.instance[0]);
 init_error=-1;assert(!osg_store_available()&&!osg_store_save(&original)&&!osg_store_forget());init_error=0;
 write_error=-1;assert(!osg_store_save(&original));write_error=0;
 commit_error=-1;assert(!osg_store_save(&original));commit_error=0;
 assert(osg_store_forget()&&!osg_store_load(&restored));assert(osg_store_forget());
 puts("Setup persistence, mode restore, ephemeral assignment, corruption and NVS failure tests passed");
}
