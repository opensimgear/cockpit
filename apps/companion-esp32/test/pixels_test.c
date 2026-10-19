#include "../network/pixels.c"
#include <assert.h>
#include <stdio.h>
static unsigned char flash[0x260000];
static const esp_partition_t part={sizeof(flash)};
static bool available=true,fail_write=false,fail_map=false;
static unsigned erased,unmapped;
const esp_partition_t *esp_partition_find_first(int type,int subtype,const char *label){assert(type==1&&subtype==0x40&&!strcmp(label,"images"));return available?&part:NULL;}
int esp_partition_read(const esp_partition_t *p,size_t offset,void *out,size_t n){assert(p==&part&&offset+n<=sizeof(flash));memcpy(out,flash+offset,n);return 0;}
int esp_partition_write(const esp_partition_t *p,size_t offset,const void *in,size_t n){assert(p==&part&&offset+n<=sizeof(flash));if(fail_write)return -1;memcpy(flash+offset,in,n);return 0;}
int esp_partition_erase_range(const esp_partition_t *p,size_t offset,size_t n){assert(p==&part&&offset+n<=sizeof(flash)&&offset%4096==0&&n%4096==0);memset(flash+offset,255,n);erased++;return 0;}
int esp_partition_mmap(const esp_partition_t *p,size_t offset,size_t n,int mode,const void **out,esp_partition_mmap_handle_t *handle){assert(p==&part&&offset+n<=sizeof(flash)&&mode==0);if(fail_map)return -1;*out=flash+offset;*handle=(unsigned)offset;return 0;}
void esp_partition_munmap(esp_partition_mmap_handle_t handle){assert(handle<sizeof(flash));unmapped++;}
int main(void){
    const char *hash="sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";bool cached=false;unsigned char data[]={0,248,255,224,7,255};
    assert(!osg_pixels_begin(2,hash,6,&cached));assert(!osg_pixels_begin(0,hash,SLOT_BYTES,&cached));
    available=false;assert(!osg_pixels_begin(0,hash,6,&cached));available=true;
    assert(osg_pixels_begin(0,hash,6,&cached)&&!cached);assert(osg_pixels_write(0,0,data,sizeof(data)));const unsigned char *a=osg_pixels_finish(0,hash,6);assert(a&&!memcmp(a,data,6));
    assert(osg_pixels_begin(1,hash,6,&cached)&&!cached);assert(osg_pixels_write(1,0,data,sizeof(data)));assert(osg_pixels_finish(1,hash,6));assert(!memcmp(a,data,6));
    unsigned before=erased;assert(osg_pixels_begin(0,hash,6,&cached)&&cached);assert(erased==before&&unmapped==1);assert(osg_pixels_finish(0,hash,6));
    assert(osg_pixels_begin(0,hash,9,&cached)&&!cached);assert(osg_pixels_begin(0,hash,9,&cached)&&!cached); // interrupted transfer has no valid header
    fail_write=true;assert(!osg_pixels_write(0,0,data,6));assert(!osg_pixels_finish(0,hash,9));fail_write=false;
    assert(!osg_pixels_write(0,SLOT_BYTES,data,6));fail_map=true;assert(!osg_pixels_finish(0,hash,9));
    puts("Flash image cache, slot isolation, reuse and interrupted writes passed");return 0;
}
