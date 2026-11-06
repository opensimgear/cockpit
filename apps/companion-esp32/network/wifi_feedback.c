#include "platform.h"
#include "esp_timer.h"
#include "mbedtls/platform_util.h"
#include <stdatomic.h>
#include <stdlib.h>
#include <string.h>

/* The worker owns TLS and writes one raw frame at a time. Only the primary
 * network task parses JSON, owns the transfer assembler, and changes UI state. */
static atomic_bool running, stopping, pending;
static SemaphoreHandle_t consumed;
static TaskHandle_t owner;
static osg_settings trust;
static char outbound[512];
static char *buffer;
static size_t length;
static osg_transfer transfer;
static bool subscribed, assignments_only, pairing_only;

static void receive_feedback(void *unused){
    (void)unused;
    const TaskHandle_t recipient=owner;
    osg_socket socket={0};
    bool connected=!atomic_load(&stopping)&&osg_socket_open(&socket,&trust)&&
        osg_socket_write_text(&socket,outbound,strlen(outbound),500);
    mbedtls_platform_zeroize(outbound,sizeof(outbound));
    while(connected&&!atomic_load(&stopping)){
        const int ready=esp_transport_poll_read(socket.ws,500);
        if(ready<0)break;
        if(!ready)continue;
        if(!osg_socket_read_text(&socket,buffer,OSG_FRAME_BYTES,&length,esp_timer_get_time()+3000000))break;
        atomic_store(&pending,true);
        xTaskNotifyGive(recipient);
        while(atomic_load(&pending)&&!atomic_load(&stopping)){
            (void)xSemaphoreTake(consumed,pdMS_TO_TICKS(500));
        }
    }
    osg_socket_close(&socket);
    if(pairing_only&&buffer)mbedtls_platform_zeroize(buffer,OSG_FRAME_BYTES+1);
    atomic_store(&running,false);
    xTaskNotifyGive(recipient);
    vTaskDelete(NULL);
}

void osg_wifi_feedback_stop(void){
    atomic_store(&stopping,true);
    atomic_store(&pending,false);
    if(consumed)xSemaphoreGive(consumed);
    free(transfer.bytes);
    memset(&transfer,0,sizeof(transfer));
    subscribed=false;
}

bool osg_wifi_feedback_running(void){return atomic_load(&running);}

static bool start_request(const osg_settings *settings,cJSON *request){
    if(atomic_load(&running)){cJSON_Delete(request);return false;}
    osg_wifi_feedback_stop();
    if(!consumed)consumed=xSemaphoreCreateBinary();
    if(!buffer)buffer=osg_workspace_allocate(OSG_FRAME_BYTES+1);
    if(!consumed||!buffer){cJSON_Delete(request);return false;}
    bool encoded=request&&cJSON_PrintPreallocated(request,outbound,sizeof(outbound),false);
    cJSON_Delete(request);
    if(!encoded){mbedtls_platform_zeroize(outbound,sizeof(outbound));return false;}
    osg_copy(trust.endpoint,sizeof(trust.endpoint),settings->endpoint);
    osg_copy(trust.ca,sizeof(trust.ca),settings->ca);
    osg_copy(trust.pin,sizeof(trust.pin),settings->pin);
    owner=xTaskGetCurrentTaskHandle();
    atomic_store(&stopping,false);
    atomic_store(&running,true);
    if(xTaskCreate(receive_feedback,"osg-feedback",8192,NULL,4,NULL)==pdPASS)return true;
    mbedtls_platform_zeroize(outbound,sizeof(outbound));
    atomic_store(&running,false);
    return false;
}

bool osg_wifi_feedback_start(const osg_settings *settings,const osg_live_session *session,const char *connection_id){
    if(atomic_load(&running))return false;
    assignments_only=session==NULL;pairing_only=false;
    cJSON *request=NULL;
    if(session)request=osg_feedback_subscription(session,settings->token);
    else if(osg_stable_id(connection_id)&&osg_secret(settings->token)){
        request=cJSON_CreateObject();
        if(!cJSON_AddStringToObject(request,"type","assignments-subscribe")||
            !cJSON_AddStringToObject(request,"connectionId",connection_id)||
            !cJSON_AddStringToObject(request,"token",settings->token)){
            cJSON_Delete(request);request=NULL;
        }
    }
    return start_request(settings,request);
}

bool osg_wifi_pairing_start(const osg_settings *settings,const char *request_id){
    if(atomic_load(&running)||!osg_stable_id(request_id)||!osg_secret(settings->secret))return false;
    cJSON *request=cJSON_CreateObject();
    if(!request||!cJSON_AddStringToObject(request,"type","pair-subscribe")||
        !cJSON_AddStringToObject(request,"requestId",request_id)||
        !cJSON_AddStringToObject(request,"secret",settings->secret)){
        cJSON_Delete(request);return false;
    }
    assignments_only=false;pairing_only=true;
    return start_request(settings,request);
}

cJSON *osg_wifi_feedback_take(void){
    if(atomic_load(&stopping)||!atomic_load(&pending))return NULL;
    cJSON *frame=osg_json_parse(buffer,length);
    if(pairing_only){
        // The host sends exactly one terminal reply, without a pending ack.
        // Consume its raw bytes before allowing another worker handoff.
        mbedtls_platform_zeroize(buffer,length);
        osg_wifi_feedback_stop();return frame;
    }
    const char *type=osg_string(frame,"type");
    if(type&&!strcmp(type,"transfer-start")&&!transfer.active){
        const double bytes=cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(frame,"bytes"));
        const size_t limit=osg_workspace_rich()?osg_transport_reply_capacity:OSG_REPLY_BYTES;
        if(bytes>=1&&bytes<=limit){
            transfer.capacity=(size_t)bytes;
            transfer.bytes=osg_workspace_allocate(transfer.capacity);
            if(!transfer.bytes)transfer.capacity=0;
        }
    }
    cJSON *message=NULL;
    int result=osg_reply_feed_object(&transfer,frame,osg_platform_crypto,&message);
    if(result!=0){
        free(transfer.bytes);
        memset(&transfer,0,sizeof(transfer));
    }
    atomic_store(&pending,false);
    xSemaphoreGive(consumed);
    if(result<0){osg_wifi_feedback_stop();return NULL;}
    if(!message)return NULL;
    if(!subscribed){
        const char *type=osg_string(message,"type");
        subscribed=assignments_only?
            cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(message,"ok"))&&type&&!strcmp(type,"assignments-subscribed"):
            osg_feedback_subscribed(message);
        cJSON_Delete(message);
        if(!subscribed)osg_wifi_feedback_stop();
        return NULL;
    }
    return message;
}
