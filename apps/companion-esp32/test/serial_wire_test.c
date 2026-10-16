#include <stdio.h>
#include <stddef.h>
static int fake_getchar(void);static int fake_puts(const char *);static int fake_fflush(FILE *);
#define getchar fake_getchar
#define puts fake_puts
#define fflush fake_fflush
#include "../network/serial_wire.c"
#undef getchar
#undef puts
#undef fflush
#include <assert.h>
#include <stdlib.h>
static const char *input="";static size_t input_at;static int64_t clock_us;static bool put_error,flush_error;static unsigned waits;
char osg_transport_frame[OSG_FRAME_BYTES+1],osg_transport_reply[OSG_REPLY_BYTES];
static int fake_getchar(void){clock_us+=10;return input[input_at]?(unsigned char)input[input_at++]:EOF;}
static int fake_puts(const char *value){assert(value&&strlen(value)<8192);return put_error?-1:0;}
static int fake_fflush(FILE *stream){assert(stream==stdout);return flush_error?-1:0;}
int64_t esp_timer_get_time(void){return clock_us;}
void vTaskDelay(unsigned ms){clock_us+=(int64_t)ms*1000;waits++;}
void esp_fill_random(void *p,size_t n){memset(p,1,n);}
void mbedtls_platform_zeroize(void *p,size_t n){memset(p,0,n);}
static bool hash(const void *p,size_t n,char out[72]){(void)p;(void)n;(void)out;return false;}
static bool decode(const char *s,unsigned char *p,size_t n,size_t *count){(void)s;(void)p;(void)n;(void)count;return false;}
const osg_crypto osg_platform_crypto={hash,decode};
static const char *valid="{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":{\"ok\":true,\"type\":\"ack\"}}\n";
static bool call(const char *bytes,cJSON **reply){osg_socket socket={.direct=true};assert(osg_copy(socket.link,sizeof(socket.link),"usb-proof"));input=bytes;input_at=0;return osg_serial_request(&socket,cJSON_CreateObject(),reply,5000);}
static void changed(const char *key,const char *value,osg_serial_failure failure){
    cJSON *root=osg_json_parse(valid,strlen(valid));assert(root);assert(cJSON_ReplaceItemInObjectCaseSensitive(root,key,cJSON_CreateString(value)));char *text=cJSON_PrintUnformatted(root);assert(text);cJSON_Delete(root);char line[2048];snprintf(line,sizeof(line),"%s\n",text);cJSON_free(text);cJSON *reply=NULL;assert(!call(line,&reply)&&!reply);(void)failure;
}
int main(void){
    osg_json_init();osg_diagnostics_reset();assert(osg_serial_start());osg_socket socket;assert(osg_serial_open(&socket));osg_serial_close(&socket);
    cJSON *reply=NULL;assert(call(valid,&reply)&&reply);cJSON_Delete(reply);reply=NULL;osg_serial_health h;osg_serial_health_snapshot(&h);assert(!h.invalid_json&&!h.request_timeouts);
    assert(!call("{broken}\n",&reply)&&!reply);osg_serial_health_snapshot(&h);assert(h.invalid_json==1&&!h.request_timeouts);
    changed("linkId","wrong",OSG_SERIAL_IDENTITY);changed("requestId","2",OSG_SERIAL_IDENTITY);
    assert(!call("{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":false}\n",&reply));osg_serial_health_snapshot(&h);assert(h.reply_mismatch==3);
    char local[1024];snprintf(local,sizeof(local),"{\"command\":\"status\"}\n%s",valid);assert(call(local,&reply));cJSON_Delete(reply);reply=osg_serial_take_command();assert(reply&&!strcmp(osg_string(reply,"command"),"status"));cJSON_Delete(reply);assert(!osg_serial_take_command());
    snprintf(local,sizeof(local),"{\"command\":\"status\"}\n{\"command\":\"status\"}\n%s",valid);assert(!call(local,&reply)&&!reply);osg_serial_close(&socket);osg_serial_health_snapshot(&h);assert(h.deferred_full==1);
    assert(!call("{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":{\"type\":\"transfer-end\",\"id\":\"unknown\"}}\n",&reply));osg_serial_health_snapshot(&h);assert(h.invalid_reply==1);
    assert(!call("{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":{\"ok\":false,\"type\":\"error\",\"error\":\"expected bench rejection\"}}\n",&reply)&&reply);cJSON_Delete(reply);reply=NULL;
    assert(!call("{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":{\"ok\":1}}\n",&reply)&&reply);cJSON_Delete(reply);reply=NULL;
    assert(!call("{\"command\":\"serial-reply\",\"linkId\":\"usb-proof\",\"requestId\":\"1\",\"frame\":{\"type\":\"ack\"}}\n",&reply)&&reply);cJSON_Delete(reply);reply=NULL;osg_serial_health_snapshot(&h);assert(h.invalid_reply==3);
    char *over=malloc(8195);assert(over);memset(over,'x',8193);over[8193]='\n';over[8194]=0;assert(!call(over,&reply)&&!reply);free(over);osg_serial_health_snapshot(&h);assert(h.frame_overflows==1);
    assert(!call("",&reply)&&!reply);assert(!call("{partial",&reply)&&!reply);osg_serial_health_snapshot(&h);assert(h.request_timeouts==2&&waits>0);
    put_error=true;assert(!call(valid,&reply)&&!reply);put_error=false;flush_error=true;assert(!call(valid,&reply)&&!reply);flush_error=false;
    socket.serial_sequence=0;assert(osg_copy(socket.link,sizeof(socket.link),"usb-proof"));cJSON *large=cJSON_CreateObject();char *huge=malloc(9001);assert(huge);memset(huge,'x',9000);huge[9000]=0;cJSON_AddStringToObject(large,"oversized",huge);free(huge);assert(!osg_serial_request(&socket,large,&reply,1));osg_serial_health_snapshot(&h);assert(h.write_failures==3);
    socket.serial_sequence=UINT64_MAX;assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,1));
    for(size_t i=0;i<sizeof(osg_transport_frame);i++){assert(!osg_transport_frame[i]);}for(size_t i=0;i<sizeof(osg_transport_reply);i++){assert(!osg_transport_reply[i]);}assert(!osg_json_allocated());
    puts("Serial reply success, correlation, malformed/oversized frames, deferred collision, timeout and write failure counters passed");return 0;
}
