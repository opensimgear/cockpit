#include "platform.h"
#include "esp_partition.h"
#include <string.h>
#define SLOT_BYTES 0x130000
#define HEADER_BYTES 4096
static esp_partition_mmap_handle_t handles[2];
static bool mapped[2], cached_slot[2];
static const unsigned char *addresses[2];
static const esp_partition_t *partition(void){return esp_partition_find_first(ESP_PARTITION_TYPE_DATA,0x40,"images");}
bool osg_pixels_begin(unsigned slot,const char *hash,size_t bytes,bool *cached){
    const esp_partition_t *p=partition();if(slot>1||!p||!hash||strlen(hash)!=71||bytes>SLOT_BYTES-HEADER_BYTES)return false;
    if(mapped[slot]){esp_partition_munmap(handles[slot]);mapped[slot]=false;}
    unsigned char header[80]={0};const size_t base=slot*SLOT_BYTES;
    *cached=esp_partition_read(p,base,header,sizeof(header))==ESP_OK && !memcmp(header,"OSGI",4) && !memcmp(header+8,hash,72);
    uint32_t saved=0;memcpy(&saved,header+4,4);*cached=*cached && saved==bytes;cached_slot[slot]=*cached;
    if(!*cached && esp_partition_erase_range(p,base,HEADER_BYTES+((bytes+4095)/4096)*4096)!=ESP_OK)return false;
    return true;
}
bool osg_pixels_write(unsigned slot,size_t offset,const void *bytes,size_t count){
    const esp_partition_t *p=partition();return slot<2 && p && offset<=SLOT_BYTES-HEADER_BYTES && count<=SLOT_BYTES-HEADER_BYTES-offset && esp_partition_write(p,slot*SLOT_BYTES+HEADER_BYTES+offset,bytes,count)==ESP_OK;
}
const unsigned char *osg_pixels_finish(unsigned slot,const char *hash,size_t bytes){
    const esp_partition_t *p=partition();if(slot>1||!p||bytes>SLOT_BYTES-HEADER_BYTES)return NULL;
    if(!cached_slot[slot]){unsigned char header[80]={0};memcpy(header,"OSGI",4);uint32_t size=(uint32_t)bytes;memcpy(header+4,&size,4);memcpy(header+8,hash,72);if(esp_partition_write(p,slot*SLOT_BYTES,header,sizeof(header))!=ESP_OK)return NULL;}
    const void *address=NULL;if(esp_partition_mmap(p,slot*SLOT_BYTES+HEADER_BYTES,bytes,ESP_PARTITION_MMAP_DATA,&address,&handles[slot])!=ESP_OK)return NULL;
    addresses[slot]=address;mapped[slot]=true;return addresses[slot];
}
