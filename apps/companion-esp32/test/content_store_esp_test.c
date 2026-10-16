#include "../render/content_store_esp.c"
#include <assert.h>
#include <stdio.h>
static esp_partition_t partition={0x190000,0x200000,false};
static bool missing,failed,init_failed;static unsigned writes,erases;
const esp_partition_t *esp_partition_find_first(int type,int subtype,const char *label){assert(type==ESP_PARTITION_TYPE_DATA&&subtype==0x40&&!strcmp(label,"ui_store"));return missing?NULL:&partition;}
int esp_partition_read(const esp_partition_t *p,size_t offset,void *bytes,size_t n){assert(p==&partition&&offset==20&&n==4);memset(bytes,42,n);return failed?-1:ESP_OK;}
int esp_partition_write(const esp_partition_t *p,size_t offset,const void *bytes,size_t n){assert(p==&partition&&offset==20&&n==4&&bytes);writes++;return failed?-1:ESP_OK;}
int esp_partition_erase_range(const esp_partition_t *p,size_t offset,size_t n){assert(p==&partition&&offset==0&&n==4096);erases++;return failed?-1:ESP_OK;}
void mbedtls_sha256_init(mbedtls_sha256_context *c){c->initialized=1;}
void mbedtls_sha256_free(mbedtls_sha256_context *c){assert(c->initialized);c->initialized=0;}
int mbedtls_sha256_starts(mbedtls_sha256_context *c,int is224){assert(c->initialized&&!is224);return failed?-1:0;}
int mbedtls_sha256_update(mbedtls_sha256_context *c,const void *bytes,size_t n){assert(c->initialized&&bytes&&n==4);return failed?-1:0;}
int mbedtls_sha256_finish(mbedtls_sha256_context *c,unsigned char out[32]){assert(c->initialized);memset(out,42,32);return failed?-1:0;}
osg_content_status osg_content_init(osg_content_store *s,osg_content_io io){s->io=io;return init_failed?OSG_CONTENT_ARGUMENT:OSG_CONTENT_OK;}
static bool validate(void *context,const osg_content_view *view,const osg_content_candidate *candidate){(void)context;(void)view;(void)candidate;return true;}
int main(void){
    osg_content_store store;osg_content_esp adapter={0};const char *sunton="sunton-3248s035r";
    assert(osg_content_esp_open(NULL,&store,sunton,NULL,validate,validate)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_esp_open(&adapter,NULL,sunton,NULL,validate,validate)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_esp_open(&adapter,&store,NULL,NULL,validate,validate)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_esp_open(&adapter,&store,sunton,NULL,NULL,validate)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_esp_open(&adapter,&store,sunton,NULL,validate,NULL)==OSG_CONTENT_ARGUMENT);
    assert(osg_content_esp_open(&adapter,&store,"headless",NULL,validate,validate)==OSG_CONTENT_NOT_FOUND);
    missing=true;assert(osg_content_esp_open(&adapter,&store,sunton,NULL,validate,validate)==OSG_CONTENT_NOT_FOUND);missing=false;
    partition.address++;assert(osg_content_esp_open(&adapter,&store,sunton,NULL,validate,validate)==OSG_CONTENT_INVALID);partition.address--;
    partition.size++;assert(osg_content_esp_open(&adapter,&store,sunton,NULL,validate,validate)==OSG_CONTENT_INVALID);partition.size--;
    partition.encrypted=true;assert(osg_content_esp_open(&adapter,&store,sunton,NULL,validate,validate)==OSG_CONTENT_INVALID);partition.encrypted=false;
    assert(!writes&&!erases);assert(osg_content_esp_open(&adapter,&store,sunton,(void *)1,validate,validate)==OSG_CONTENT_OK);
    assert(store.io.bytes==0x200000&&store.io.manifest_limit==4096&&store.io.package_limit==768*1024&&store.io.validation_context==(void *)1);
    uint8_t bytes[32];for(unsigned failure=0;failure<2;failure++){failed=failure;assert(store.io.read(store.io.context,20,bytes,4)==!failed);assert(store.io.write(store.io.context,20,bytes,4)==!failed);assert(store.io.erase(store.io.context,0,4096)==!failed);
        assert(store.io.hash_begin(store.io.hash_context)==!failed);assert(store.io.hash_update(store.io.hash_context,bytes,4)==!failed);assert(store.io.hash_finish(store.io.hash_context,bytes)==!failed);}
    osg_content_esp_close(&adapter);osg_content_esp_close(&adapter);osg_content_esp_close(NULL);failed=false;partition.size=0x600000;
    assert(osg_content_esp_open(&adapter,&store,"waveshare-lcd7-s3",NULL,validate,validate)==OSG_CONTENT_OK);assert(store.io.bytes==0x600000&&store.io.manifest_limit==8192&&store.io.package_limit==2560*1024);osg_content_esp_close(&adapter);
    init_failed=true;assert(osg_content_esp_open(&adapter,&store,"waveshare-lcd7-s3",NULL,validate,validate)==OSG_CONTENT_ARGUMENT&&!adapter.initialized);
    puts("ESP content partition identity/encryption gates, bounded profile sizes and SHA/I/O error propagation passed");
}
