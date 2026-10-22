/* Software acceptance target: the actual firmware client, source banks, input
 * controller and LVGL compositor. A local test bridge supplies authenticated WSS
 * I/O and a bounded memory image cache. No ESP-IDF scheduling, flash durability,
 * physical touch, DMA, native Android authentication or hardware claim is made. */
#define _DEFAULT_SOURCE
#include "panel.h"
#include <stdio.h>
#include <limits.h>
#include <sys/time.h>
#include <time.h>
static int no_serial_byte(void){return EOF;}
static int no_clock_write(const struct timeval *value,const struct timezone *zone){(void)value;(void)zone;return -1;}
#define getchar no_serial_byte
#define settimeofday no_clock_write
#include "../network/client.c"
#undef getchar
#undef settimeofday
#include "panel_draw.h"
#include <assert.h>
#include <openssl/evp.h>
#include <openssl/rand.h>
#include <openssl/sha.h>

static osg_runtime screen;
static osg_panel_draw renderer;
static const osg_panel *borrowed;
static bool dirty,pointer_down,pointer_valid=true;
static int pointer_x,pointer_y;
static uint16_t pixels[800*480],partial[800*4] __attribute__((aligned(64)));
static unsigned frames,writes;

int64_t esp_timer_get_time(void){struct timespec at;assert(!clock_gettime(CLOCK_MONOTONIC,&at));return (int64_t)at.tv_sec*1000000+at.tv_nsec/1000;}
void vTaskDelay(unsigned ms){struct timespec delay={ms/1000,(long)(ms%1000)*1000000};nanosleep(&delay,NULL);}
int xTaskCreate(void (*fn)(void *),const char *name,unsigned bytes,void *arg,unsigned priority,void *handle){(void)fn;(void)name;(void)bytes;(void)arg;(void)priority;(void)handle;return pdPASS;}
int xSemaphoreTake(SemaphoreHandle_t mutex,unsigned wait){(void)mutex;(void)wait;return 1;}
int xSemaphoreGive(SemaphoreHandle_t mutex){(void)mutex;return 1;}
void esp_fill_random(void *pointer,size_t bytes){assert(bytes<=INT_MAX&&RAND_bytes(pointer,(int)bytes)==1);}
void mbedtls_platform_zeroize(void *pointer,size_t bytes){memset(pointer,0,bytes);}
bool osg_workspace_rich(void){return true;}
void *osg_workspace_allocate(size_t bytes){return malloc(bytes);}
size_t osg_workspace_available(void){return 0; /* Native malloc has no largest-free-block query. */}
static bool sha(const void *data,size_t bytes,char out[72]){unsigned char digest[32];SHA256(data,bytes,digest);memcpy(out,"sha256:",7);for(unsigned i=0;i<32;i++)snprintf(out+7+i*2,3,"%02x",digest[i]);return true;}
static bool decode(const char *source,unsigned char *out,size_t capacity,size_t *size){
    size_t length=strlen(source);if(!length||length%4||length>8192)return false;
    unsigned char bytes[8192];int count=EVP_DecodeBlock(bytes,(const unsigned char *)source,(int)length);if(count<0)return false;
    if(source[length-1]=='=')count--;
    if(source[length-2]=='=')count--;
    if((size_t)count>capacity)return false;
    memcpy(out,bytes,(size_t)count);*size=(size_t)count;return true;
}
const osg_crypto osg_platform_crypto={sha,decode};
cJSON *osg_serial_take_command(void){return NULL;}
cJSON *osg_wifi_scan(void){return cJSON_CreateArray();}
bool osg_serial_start(void){return true;}
bool osg_wifi_clock(void){return true;}
bool osg_wifi_start(const osg_settings *value){(void)value;return true;}
void osg_wifi_stop(void){}
bool osg_trusted_ca(const osg_settings *value,unsigned char *bytes,size_t capacity,size_t *size){(void)value;(void)bytes;(void)capacity;(void)size;return false;}
bool osg_store_available(void){return false;}
bool osg_store_save(const osg_settings *value){(void)value;return false;}
bool osg_store_load(osg_settings *value){(void)value;return false;}
bool osg_store_forget(void){return true;}

static cJSON *record(size_t capacity){
    char *line=malloc(capacity+2);assert(line);cJSON *value=NULL;
    if(fgets(line,(int)capacity+2,stdin)&&strchr(line,'\n'))value=osg_json_parse(line,strlen(line));
    free(line);return value;
}
static cJSON *bridge(const char *kind,cJSON *frame){
    cJSON *request=cJSON_CreateObject();assert(request&&cJSON_AddStringToObject(request,"bridge",kind));
    if(frame)assert(cJSON_AddItemToObject(request,"frame",frame));
    char *encoded=cJSON_PrintUnformatted(request);assert(encoded);puts(encoded);fflush(stdout);cJSON_free(encoded);cJSON_Delete(request);
    return record(osg_render_profile_limits()->reply_bytes);
}
bool osg_socket_open(osg_socket *socket,const osg_settings *value){
    (void)value;cJSON *reply=bridge("open",NULL);bool ok=reply&&cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(reply,"ok"));cJSON_Delete(reply);if(ok)socket->ws=socket;return ok;
}
void osg_socket_close(osg_socket *socket){if(socket->ws){cJSON *reply=bridge("close",NULL);cJSON_Delete(reply);}memset(socket,0,sizeof(*socket));}
bool osg_socket_request(osg_socket *socket,cJSON *frame,cJSON **reply,unsigned timeout){
    (void)socket;(void)timeout;*reply=bridge("request",frame);return *reply&&cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(*reply,"ok"));
}

static struct {char hash[72];unsigned width,height,pins;size_t size,written;unsigned char *pixels;bool complete;} images[32];
static size_t cache_bytes;
void osg_pixels_panel_release(unsigned owner){for(unsigned i=0;i<32;i++)images[i].pins&=~(1u<<owner);}
bool osg_pixels_panel_begin(unsigned owner){osg_pixels_panel_release(owner);return true;}
static int image_find(const char *hash,unsigned width,unsigned height,size_t bytes){for(unsigned i=0;i<32;i++)if(images[i].pixels&&!strcmp(images[i].hash,hash)&&images[i].width==width&&images[i].height==height&&images[i].size==bytes)return (int)i;return -1;}
bool osg_pixels_contains(const char *hash,unsigned width,unsigned height,size_t bytes){int i=image_find(hash,width,height,bytes);return i>=0&&images[i].complete;}
bool osg_pixels_acquire(unsigned owner,const char *hash,unsigned width,unsigned height,size_t bytes,unsigned *asset,bool *cached){
    int i=image_find(hash,width,height,bytes);*cached=i>=0&&images[i].complete;
    if(i<0){
        for(unsigned at=0;at<32;at++)if(!images[at].pins){i=(int)at;break;}
        if(i<0)return false;
        cache_bytes-=images[i].size;free(images[i].pixels);memset(&images[i],0,sizeof(images[i]));
        if(bytes>osg_render_profile_limits()->cache_bytes-cache_bytes)return false;
        images[i].pixels=malloc(bytes);if(!images[i].pixels)return false;
        images[i].size=bytes;images[i].width=width;images[i].height=height;osg_copy(images[i].hash,sizeof(images[i].hash),hash);cache_bytes+=bytes;
    }
    if(!images[i].complete)images[i].written=0;
    images[i].pins|=1u<<owner;*asset=(unsigned)i;return true;
}
bool osg_pixels_write(unsigned asset,size_t offset,const void *bytes,size_t count){if(asset>=32||offset!=images[asset].written||count>images[asset].size-offset)return false;memcpy(images[asset].pixels+offset,bytes,count);images[asset].written+=count;return true;}
const unsigned char *osg_pixels_finish(unsigned asset){if(asset>=32||images[asset].written!=images[asset].size)return NULL;images[asset].complete=true;return images[asset].pixels;}
const char *osg_pixels_error(void){return "cache_full";}
static bool write_pixels(void *context,int x,int y,int width,int height,const uint16_t *data){
    (void)context;if(x<0||y<0||width<1||height<1||x+width>800||y+height>480||width*height>800*4)return false;
    for(int row=0;row<height;row++)memcpy(pixels+(y+row)*800+x,data+row*width,(size_t)width*2);
    writes++;return true;
}
static bool present(void *context){(void)context;frames++;return true;}
static void initial_fill(void *context,int x,int y,int width,int height,uint16_t color){(void)context;(void)x;(void)y;(void)width;(void)height;(void)color;}
static void initial_text(void *context,int x,int y,int width,int height,const char *text,uint16_t color){(void)text;initial_fill(context,x,y,width,height,color);}
static void tick(void){
    osg_network_touch(pointer_valid,pointer_down,pointer_x,pointer_y);update_screen();advance();update_screen();
    osg_scene_ticket ticket;const osg_panel *candidate=osg_network_candidate_begin(&ticket);
    if(candidate){
        osg_runtime drawing;osg_panel_source_diagnostic diagnostic={0};bool ok=osg_init(&drawing,candidate,screen.display,NULL,NULL);
        if(ok)ok=osg_panel_draw_check(&renderer,&drawing,millis(),true);
        if(!ok){diagnostic.code=OSG_PANEL_SOURCE_UNSUPPORTED;osg_copy(diagnostic.widget,sizeof(diagnostic.widget),renderer.diagnostic.widget);osg_copy(diagnostic.layer,sizeof(diagnostic.layer),renderer.diagnostic.layer);osg_copy(diagnostic.field,sizeof(diagnostic.field),renderer.diagnostic.field);diagnostic.required=renderer.diagnostic.required;diagnostic.available=renderer.diagnostic.available;}
        osg_network_candidate_finish(ticket,ok,&diagnostic);
    }
    if(state==LIVE){
        osg_runtime drawing=screen;borrowed=drawing.panel;osg_network_render(drawing.panel,false,true);
        bool ok=osg_panel_draw_render(&renderer,&drawing,(osg_damage){.full=true},millis());borrowed=NULL;osg_network_render(drawing.panel,true,ok);
        assert(ok);
    }
    osg_network_touch(pointer_valid,pointer_down,pointer_x,pointer_y);arm_when_ready();
}
static cJSON *snapshot(void){
    cJSON *value=cJSON_CreateObject();assert(value);
    cJSON_AddStringToObject(value,"phase",state==LIVE&&screen.armed?"live-armed":phases[state]);cJSON_AddBoolToObject(value,"armed",screen.armed);
    cJSON_AddStringToObject(value,"publicationHash",screen.panel->publication_hash);cJSON_AddNumberToObject(value,"frames",frames);cJSON_AddNumberToObject(value,"writes",writes);cJSON_AddNumberToObject(value,"widgets",screen.panel->widget_count);
    cJSON_AddNumberToObject(value,"processId",getpid());
    static const char *names[]={"normal","active","pressed","warning","stale","unknown","disabled","disconnected"};
    cJSON *appearance=cJSON_AddArrayToObject(value,"appearance");
    for(size_t i=0;i<screen.panel->widget_count;i++)if(screen.panel->widgets[i].kind==OSG_COMPONENT){
        osg_widget_state_context context;osg_widget_runtime_context(&screen,i,millis(),&context);
        cJSON *item=cJSON_CreateObject();cJSON_AddStringToObject(item,"widget",screen.panel->widgets[i].id);cJSON_AddStringToObject(item,"state",names[osg_widget_state_resolve(screen.panel->widgets[i].component->definition,&context)]);cJSON_AddItemToArray(appearance,item);
    }
    add_operation(value);return value;
}
int main(void){
    setvbuf(stdout,NULL,_IOLBF,0);osg_json_init();const osg_render_limits *profile=osg_render_profile_limits();assert(osg_json_configure(profile->json_bytes,profile->reply_bytes,profile->json_nodes));
    assert(osg_init(&screen,&osg_demo_panel,(osg_display){.width=800,.height=480,.fill=initial_fill,.text=initial_text},NULL,NULL));
    assert(osg_panel_draw_init(&renderer,(osg_board){.width=800,.height=480,.write=write_pixels,.present=present},partial,sizeof(partial)));renderer.max_font_size=(uint8_t)profile->font_size;
    osg_network_start((osg_network_ui){&screen,(void *)1,&dirty,&borrowed,true});
    for(;;){
        cJSON *request=record(4096);if(!request)break;const char *kind=osg_string(request,"command");bool quit=equal(kind,"quit");
        if(equal(kind,"install")){
            stop();assert(osg_copy(settings.host,sizeof(settings.host),osg_string(request,"hostId"))&&osg_copy(settings.companion,sizeof(settings.companion),osg_string(request,"companionId"))&&osg_copy(settings.token,sizeof(settings.token),osg_string(request,"token")));state=AUTH;next_request=millis();
        }else if(equal(kind,"reconnect")){stop();state=AUTH;next_request=millis();}
        else if(equal(kind,"step"))tick();
        else if(equal(kind,"touch")){pointer_valid=!cJSON_IsFalse(cJSON_GetObjectItemCaseSensitive(request,"valid"));pointer_down=cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(request,"down"));pointer_x=(int)cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(request,"x"));pointer_y=(int)cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(request,"y"));osg_network_touch(pointer_valid,pointer_down,pointer_x,pointer_y);}
        else if(equal(kind,"snapshot")){
            FILE *output=fopen(osg_string(request,"path"),"wb");assert(output);fprintf(output,"P6\n800 480\n255\n");for(unsigned i=0;i<800*480;i++){unsigned char rgb[]={(unsigned char)((pixels[i]>>11)*255/31),(unsigned char)(((pixels[i]>>5&63)*255)/63),(unsigned char)((pixels[i]&31)*255/31)};assert(fwrite(rgb,1,3,output)==3);}fclose(output);
        }else if(!quit){assert(equal(kind,"status"));}
        cJSON *reply=snapshot();cJSON_AddItemToObject(reply,"requestId",cJSON_Duplicate(cJSON_GetObjectItemCaseSensitive(request,"requestId"),true));
        if(equal(kind,"status"))cJSON_AddItemToObject(reply,"renderProfile",osg_render_profile_json(800,480));
        char *encoded=cJSON_PrintUnformatted(reply);assert(encoded);puts(encoded);cJSON_free(encoded);cJSON_Delete(reply);cJSON_Delete(request);if(quit)break;
    }
    stop();osg_scene_banks_destroy(&banks);osg_panel_draw_destroy(&renderer);for(unsigned i=0;i<32;i++)free(images[i].pixels);assert(!osg_json_allocated());return 0;
}
