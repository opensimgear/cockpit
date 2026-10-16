#include "content_store_esp.h"
#include <string.h>
static bool read_partition(void *context,uint32_t offset,void *bytes,size_t n){osg_content_esp *a=context;return esp_partition_read(a->partition,offset,bytes,n)==ESP_OK;}
static bool write_partition(void *context,uint32_t offset,const void *bytes,size_t n){osg_content_esp *a=context;return esp_partition_write(a->partition,offset,bytes,n)==ESP_OK;}
static bool erase_partition(void *context,uint32_t offset,uint32_t n){osg_content_esp *a=context;return esp_partition_erase_range(a->partition,offset,n)==ESP_OK;}
static bool sha_begin(void *context){return mbedtls_sha256_starts(context,0)==0;}
static bool sha_update(void *context,const void *bytes,size_t n){return mbedtls_sha256_update(context,bytes,n)==0;}
static bool sha_finish(void *context,uint8_t out[32]){return mbedtls_sha256_finish(context,out)==0;}
osg_content_status osg_content_esp_open(osg_content_esp *a,osg_content_store *store,const char *profile,void *validation_context,
    bool (*manifest)(void *,const osg_content_view *,const osg_content_candidate *),bool (*content)(void *,const osg_content_view *,const osg_content_candidate *)){
    if(!a||!store||!profile||!manifest||!content)return OSG_CONTENT_ARGUMENT;
    uint32_t expected_size=0,manifest_limit=0,package_limit=0;
    if(!strcmp(profile,"sunton-3248s035r")){expected_size=0x200000;manifest_limit=4096;package_limit=768*1024;}
    else if(!strcmp(profile,"waveshare-lcd7-s3")){expected_size=0x600000;manifest_limit=8192;package_limit=2560*1024;}
    else return OSG_CONTENT_NOT_FOUND;
    memset(a,0,sizeof(*a));a->partition=esp_partition_find_first(ESP_PARTITION_TYPE_DATA,0x40,"ui_store");
    if(!a->partition)return OSG_CONTENT_NOT_FOUND;
    if(a->partition->address!=0x190000||a->partition->size!=expected_size||a->partition->encrypted)return OSG_CONTENT_INVALID;
    mbedtls_sha256_init(&a->hash);a->initialized=true;
    osg_content_io io={.context=a,.bytes=expected_size,.erase_bytes=4096,.manifest_limit=manifest_limit,.package_limit=package_limit,
        .read=read_partition,.write=write_partition,.erase=erase_partition,.hash_context=&a->hash,.hash_begin=sha_begin,.hash_update=sha_update,.hash_finish=sha_finish,
        .validation_context=validation_context,.validate_manifest=manifest,.validate_content=content};
    osg_content_status status=osg_content_init(store,io);if(status!=OSG_CONTENT_OK)osg_content_esp_close(a);return status;
}
void osg_content_esp_close(osg_content_esp *a){if(a&&a->initialized){mbedtls_sha256_free(&a->hash);memset(a,0,sizeof(*a));}}
