/* Bounded USB serial transport for the existing companion protocol. */
#include "platform.h"
#include "esp_timer.h"
#include "esp_random.h"
#include "mbedtls/platform_util.h"
#include <inttypes.h>
#include <stdio.h>
#include <string.h>
extern char osg_transport_frame[OSG_FRAME_BYTES+1], osg_transport_reply[OSG_REPLY_BYTES];
static cJSON *deferred;
cJSON *osg_serial_take_command(void){cJSON *v=deferred;deferred=NULL;return v;}
bool osg_serial_open(osg_socket *s){
    unsigned char random[8];esp_fill_random(random,sizeof(random));
    memset(s,0,sizeof(*s));s->direct=true;
    snprintf(s->link,sizeof(s->link),"usb-%02x%02x%02x%02x%02x%02x%02x%02x",random[0],random[1],random[2],random[3],random[4],random[5],random[6],random[7]);
    printf("{\"type\":\"serial-open\",\"linkId\":\"%s\"}\n",s->link);fflush(stdout);return true;
}
void osg_serial_close(osg_socket *s){
    if(s->direct){printf("{\"type\":\"serial-close\",\"linkId\":\"%s\"}\n",s->link);fflush(stdout);}
    cJSON_Delete(deferred);deferred=NULL;
}
bool osg_serial_request(osg_socket *s,cJSON *request,cJSON **response,unsigned timeout){
    *response=NULL;
    if(s->serial_sequence==UINT64_MAX){cJSON_Delete(request);return false;}
    char id[21],outbound[8192];
    snprintf(id,sizeof(id),"%" PRIu64,++s->serial_sequence);
    cJSON *v=cJSON_CreateObject();cJSON_AddStringToObject(v,"type","serial-request");
    cJSON_AddStringToObject(v,"linkId",s->link);cJSON_AddStringToObject(v,"requestId",id);cJSON_AddItemToObject(v,"frame",request);
    bool ok=cJSON_PrintPreallocated(v,outbound,sizeof(outbound),false);cJSON_Delete(v);
    if(!ok){mbedtls_platform_zeroize(outbound,sizeof(outbound));return false;}
    puts(outbound);fflush(stdout);mbedtls_platform_zeroize(outbound,sizeof(outbound));
    memset(osg_transport_reply,0,OSG_REPLY_BYTES);
    osg_transfer transfer={.bytes=osg_transport_reply,.capacity=OSG_REPLY_BYTES};
    size_t used=0;bool overflow=false;
    const int64_t deadline=esp_timer_get_time()+(int64_t)timeout*1000;
    while(esp_timer_get_time()<deadline){
        int c=getchar();
        if(c==EOF){clearerr(stdin);vTaskDelay(pdMS_TO_TICKS(1));continue;}
        if(c!='\r'&&c!='\n'){
            if(used<8192&&!overflow)osg_transport_frame[used++]=(char)c;else overflow=true;
            continue;
        }
        if(!used&&!overflow)continue;
        if(overflow)break;
        v=osg_json_parse(osg_transport_frame,used);used=0;
        if(!v)break;
        const char *command=osg_string(v,"command");
        /* Keep USB liveness responsive throughout multi-chunk panel downloads.
         * Mutating commands still wait for the outer network task. */
        if(command&&!strcmp(command,"status")){
            osg_network_status(osg_string(v,"requestId"));cJSON_Delete(v);continue;
        }
        if(!command||strcmp(command,"serial-reply")){
            if(deferred){cJSON_Delete(v);break;}
            deferred=v;continue;
        }
        const char *link=osg_string(v,"linkId"),*reply_id=osg_string(v,"requestId");
        cJSON *body=cJSON_GetObjectItemCaseSensitive(v,"frame");
        ok=link&&reply_id&&!strcmp(link,s->link)&&!strcmp(reply_id,id)&&cJSON_IsObject(body)&&
            cJSON_PrintPreallocated(body,osg_transport_frame,OSG_FRAME_BYTES+1,false);
        cJSON_Delete(v);
        if(!ok)break;
        int result=osg_reply_feed(&transfer,osg_transport_frame,strlen(osg_transport_frame),osg_platform_crypto,response);
        if(result<0)break;
        if(result==1){
            mbedtls_platform_zeroize(osg_transport_frame,OSG_FRAME_BYTES+1);
            mbedtls_platform_zeroize(osg_transport_reply,OSG_REPLY_BYTES);
            return cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(*response,"ok"));
        }
    }
    mbedtls_platform_zeroize(osg_transport_frame,OSG_FRAME_BYTES+1);
    mbedtls_platform_zeroize(osg_transport_reply,OSG_REPLY_BYTES);return false;
}
