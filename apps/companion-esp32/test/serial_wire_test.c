/* Exercise the real serial reader with status probes interleaved with replies. */
#include <stdio.h>
static int serial_getchar(void);
#define getchar serial_getchar
#include "../network/serial_wire.c"
#undef getchar
#include <assert.h>
#include <stdlib.h>
#include <openssl/sha.h>
#include <openssl/evp.h>

char osg_transport_frame[OSG_FRAME_BYTES+1];
static char reply_storage[192*1024];
char *osg_transport_reply=reply_storage;
size_t osg_transport_reply_capacity=sizeof(reply_storage);
static bool storage_ready=true;
bool osg_transport_storage_prepare(void){return storage_ready;}
static bool sha(const void *p,size_t n,char out[72]){unsigned char d[32];SHA256(p,n,d);memcpy(out,"sha256:",7);for(size_t i=0;i<32;i++)snprintf(out+7+i*2,3,"%02x",d[i]);return true;}
static bool decode(const char *s,unsigned char *out,size_t capacity,size_t *n){size_t len=strlen(s);if(!len||len%4)return false;unsigned char *tmp=malloc(len);assert(tmp);int count=EVP_DecodeBlock(tmp,(const unsigned char *)s,(int)len);if(count<0){free(tmp);return false;}if(s[len-1]=='=')count--;if(s[len-2]=='=')count--;if((size_t)count>capacity){free(tmp);return false;}memcpy(out,tmp,(size_t)count);free(tmp);*n=(size_t)count;return true;}
const osg_crypto osg_platform_crypto={sha,decode};
static const char *incoming;
static unsigned probes;
static int64_t now;
static int serial_getchar(void){return *incoming?(unsigned char)*incoming++:EOF;}
int64_t esp_timer_get_time(void){return now;}
void vTaskDelay(unsigned ms){now+=(int64_t)ms*1000;}
void esp_fill_random(void *p,size_t n){memset(p,42,n);}
void mbedtls_platform_zeroize(void *p,size_t n){memset(p,0,n);}
void osg_network_status(const char *id){
    char expected[16];snprintf(expected,sizeof(expected),"probe-%u",++probes);
    assert(id&&!strcmp(id,expected));
}
static void large_reply(osg_socket *socket,bool accepted){
    const size_t text_bytes=40*1024,capacity=96*1024;const char *prefix="{\"ok\":true,\"padding\":\"";
    char *payload=malloc(text_bytes+strlen(prefix)+3),*stream=malloc(capacity);assert(payload&&stream);
    memcpy(payload,prefix,strlen(prefix));memset(payload+strlen(prefix),'A',text_bytes);strcpy(payload+strlen(prefix)+text_bytes,"\"}");
    const size_t bytes=strlen(payload);char hash[72];assert(sha(payload,bytes,hash));
    const uint64_t id=socket->serial_sequence+1;size_t used=0;
    int n=snprintf(stream,capacity,"{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"%" PRIu64 "\",\"frame\":{\"type\":\"transfer-start\",\"id\":\"large\",\"bytes\":%zu,\"sha256\":\"%s\"}}\n",id,bytes,hash);assert(n>0);used=(size_t)n;
    for(size_t offset=0,index=0;offset<bytes;index++){
        size_t count=bytes-offset;if(count>2048)count=2048;char encoded[2733];
        assert(EVP_EncodeBlock((unsigned char *)encoded,(const unsigned char *)payload+offset,(int)count)>0);
        n=snprintf(stream+used,capacity-used,"{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"%" PRIu64 "\",\"frame\":{\"type\":\"transfer-chunk\",\"id\":\"large\",\"index\":%zu,\"base64\":\"%s\"}}\n",id,index,encoded);assert(n>0&&(size_t)n<capacity-used);used+=(size_t)n;offset+=count;
    }
    n=snprintf(stream+used,capacity-used,"{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"%" PRIu64 "\",\"frame\":{\"type\":\"transfer-end\",\"id\":\"large\"}}\n",id);assert(n>0&&(size_t)n<capacity-used);
    incoming=stream;cJSON *reply=NULL;assert(osg_serial_request(socket,cJSON_CreateObject(),&reply,3000)==accepted);
    if(accepted)assert(reply&&strlen(osg_string(reply,"padding"))==text_bytes);
    else assert(!reply&&!strcmp(socket->request_error,"Invalid USB reply data or checksum"));
    cJSON_Delete(reply);incoming="";free(stream);free(payload);
}
int main(void){
    osg_json_init();
    assert(osg_json_configure(512*1024,sizeof(reply_storage),4096));
    osg_socket socket={.direct=true};strcpy(socket.link,"test-link");
    cJSON *allocation_failure=NULL;storage_ready=false;
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&allocation_failure,3000));
    assert(!allocation_failure&&!strcmp(socket.request_error,"USB reply workspace allocation failed"));storage_ready=true;
    /* An arm command must remain deferred while any number of probes are served. */
    incoming="{\"command\":\"arm\",\"armEpoch\":\"1\"}\n"
        "{\"command\":\"status\",\"requestId\":\"probe-1\"}\n"
        "{\"command\":\"status\",\"requestId\":\"probe-2\"}\n"
        "{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"1\",\"frame\":{\"ok\":true,\"type\":\"image-chunk\",\"offset\":0}}\n";
    cJSON *reply=NULL;
    assert(osg_serial_request(&socket,cJSON_CreateObject(),&reply,3000));
    assert(probes==2&&cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"offset"))==0);
    cJSON_Delete(reply);
    /* load_images issues its next request before draining deferred commands. */
    incoming="{\"command\":\"status\",\"requestId\":\"probe-3\"}\n"
        "{\"command\":\"status\",\"requestId\":\"probe-4\"}\n"
        "{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"2\",\"frame\":{\"ok\":true,\"type\":\"image-chunk\",\"offset\":6144}}\n";
    assert(osg_serial_request(&socket,cJSON_CreateObject(),&reply,3000));
    assert(probes==4&&cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"offset"))==6144);
    cJSON_Delete(reply);
    cJSON *command=osg_serial_take_command();
    assert(command&&!strcmp(osg_string(command,"command"),"arm"));cJSON_Delete(command);
    assert(!osg_serial_take_command());
    /* Status replies must not extend the request deadline indefinitely. */
    incoming="{\"command\":\"status\",\"requestId\":\"probe-5\"}\n";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(probes==5&&!reply&&now==10000&&!strcmp(socket.request_error,"USB response timed out"));
    incoming="bad json\n";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!strcmp(socket.request_error,"Invalid USB reply JSON"));
    incoming="{\"command\":\"serial-reply\",\"linkId\":\"wrong-link\",\"requestId\":\"5\",\"frame\":{\"ok\":true}}\n";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!strcmp(socket.request_error,"USB reply identity or frame mismatch"));
    incoming="{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"6\",\"frame\":{\"ok\":false,\"type\":\"error\",\"error\":\"Credential revoked.\"}}\n";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!socket.request_error&&!strcmp(osg_string(reply,"error"),"Credential revoked."));cJSON_Delete(reply);
    incoming="{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"7\",\"frame\":{\"type\":\"transfer-end\",\"id\":\"missing-start\"}}\n";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!strcmp(socket.request_error,"Invalid USB reply data or checksum"));
    /* USB writes can be delivered in any number of reads. The outer network
     * task may have consumed any prefix of a status probe before starting its
     * heartbeat. Every handoff must keep that prefix instead of parsing only
     * the suffix as a new JSON record. */
    char probe[128],frames[512];
    unsigned fragmented=0;
    for(size_t split=1;;split++){
        snprintf(probe,sizeof(probe),"{\"command\":\"status\",\"requestId\":\"probe-%u\"}\r\n",probes+1);
        if(split>=strlen(probe)-2)break;
        for(size_t at=0;at<split;at++){
            assert(osg_serial_json_byte(probe[at],&reply)==OSG_SERIAL_INCOMPLETE&&!reply);
        }
        snprintf(frames,sizeof(frames),"%s{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"%" PRIu64 "\",\"frame\":{\"ok\":true}}\n",probe+split,socket.serial_sequence+1);
        incoming=frames;
        assert(osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
        assert(reply&&!strcmp(socket.request_error?socket.request_error:"",""));
        cJSON_Delete(reply);fragmented++;
    }
    assert(fragmented>30&&probes==5+fragmented);
    /* The reverse handoff also retains an incomplete reply if its deadline
     * expires. Idle polling can finish and discard it without corrupting the
     * next console command or heartbeat. */
    incoming="{\"command\":\"serial-reply\",\"linkId\":\"test-link\",";
    assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!reply&&!strcmp(socket.request_error,"USB response timed out"));
    const char *tail="\"requestId\":\"expired\",\"frame\":{\"ok\":true}}\n";
    for(size_t at=0;tail[at];at++){
        osg_serial_record record=osg_serial_json_byte(tail[at],&reply);
        if(tail[at+1])assert(record==OSG_SERIAL_INCOMPLETE&&!reply);
        else assert(record==OSG_SERIAL_JSON&&reply&&!strcmp(osg_string(reply,"command"),"serial-reply"));
    }
    cJSON_Delete(reply);
    /* Overflow spans the same handoff, and draining its newline resets the
     * decoder so the following bounded reply remains usable. */
    for(unsigned at=0;at<OSG_PROVISION_BYTES+1;at++)assert(osg_serial_json_byte('x',&reply)==OSG_SERIAL_INCOMPLETE&&!reply);
    incoming="\n";assert(!osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));
    assert(!reply&&!strcmp(socket.request_error,"USB reply exceeds buffer"));
    snprintf(frames,sizeof(frames),"{\"command\":\"serial-reply\",\"linkId\":\"test-link\",\"requestId\":\"%" PRIu64 "\",\"frame\":{\"ok\":true}}\n",socket.serial_sequence+1);
    incoming=frames;assert(osg_serial_request(&socket,cJSON_CreateObject(),&reply,10));cJSON_Delete(reply);
    large_reply(&socket,true);osg_transport_reply_capacity=32768;large_reply(&socket,false);osg_transport_reply_capacity=sizeof(reply_storage);
    osg_serial_close(&socket);
    assert(!osg_json_allocated());
    puts("Serial status probes remain responsive; fragmented records survive idle/request handoffs and timeouts");
}
