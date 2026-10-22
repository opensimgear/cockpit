#include "platform.h"
#include "firmware.h"
#include "input.h"
#include "widget.h"
#include "serial.h"
#include "serial_json.h"
#include "esp_timer.h"
#include "esp_random.h"
#include "mbedtls/platform_util.h"
#include <stdio.h>
#include <string.h>
#include <inttypes.h>
#include <fcntl.h>
#include <sys/time.h>
#include <unistd.h>
#include <math.h>
#include <time.h>
#include <stdlib.h>

typedef enum { OFF, PAIR_BEGIN, PAIR_POLL, AUTH, WAIT_ASSIGNMENT, OPEN, LIVE, FAULT, PANEL_ERROR } phase;
static osg_network_ui ui;
static osg_settings settings;
static osg_socket connection;
static osg_image images[2];
static unsigned image_index;
static osg_live_session session;
static osg_input input;
static bool remote_armed, installation_saved, panel_render_failed;
static uint64_t input_sequence, next_input_request;
static phase state;
static const char *detail;
static char request_detail[192],fault_detail[192];
static const char *request_label="Request";
static void update_screen(void);
static char pending[65], assigned[65], publication[72];
static uint64_t next_request, pair_deadline;
static const char *phases[]={"offline","pairing","awaiting-approval","authenticating","awaiting-assignment","opening-panel","live-disarmed","error","panel-error"};
typedef struct {
    char instance[65],publication[72],session[65],generation[21];
    const char *phase,*error;
    size_t completed,total;
    unsigned hits,misses;
    uint32_t attempt;
    bool retryable;
} panel_operation;
static panel_operation operation;
static uint32_t operation_attempt;
static uint64_t operation_reported;
static cJSON *interrupted_command;
static bool load_interrupted;
static unsigned loading_owner;
static bool capture_command;
static void serial_byte(int);
static uint64_t millis(void){return (uint64_t)(esp_timer_get_time()/1000);}
static void add_operation(cJSON *v){
    if(!operation.phase)return;
    cJSON *o=cJSON_AddObjectToObject(v,"operation");
    cJSON_AddStringToObject(o,"instanceId",operation.instance);cJSON_AddStringToObject(o,"publicationHash",operation.publication);
    cJSON_AddStringToObject(o,"phase",operation.phase);cJSON_AddNumberToObject(o,"attemptId",operation.attempt);
    cJSON_AddNumberToObject(o,"completedBytes",(double)operation.completed);cJSON_AddNumberToObject(o,"totalBytes",(double)operation.total);
    cJSON_AddNumberToObject(o,"cacheHits",operation.hits);cJSON_AddNumberToObject(o,"cacheMisses",operation.misses);
    if(operation.session[0]){cJSON_AddStringToObject(o,"sessionId",operation.session);cJSON_AddStringToObject(o,"generation",operation.generation);}
    if(operation.error){cJSON_AddStringToObject(o,"errorCode",operation.error);cJSON_AddBoolToObject(o,"retryable",operation.retryable);}
    if(detail)cJSON_AddStringToObject(o,"message",detail);
}
static void report(const char *status){
    cJSON *v=cJSON_CreateObject();char encoded[1536];cJSON_AddStringToObject(v,"status",status);
    if(detail)cJSON_AddStringToObject(v,"statusMessage",detail);
    add_operation(v);
    if(cJSON_PrintPreallocated(v,encoded,sizeof(encoded),false)){puts(encoded);fflush(stdout);}cJSON_Delete(v);operation_reported=millis();
}
static void operation_phase(const char *value){
    if(!operation.phase||strcmp(operation.phase,value)){operation.phase=value;operation.error=NULL;operation.retryable=false;report(phases[state]);}
}
static void operation_failed(const char *code,bool retryable){
    if(!operation.phase)return;
    operation.phase="failed";operation.error=code;operation.retryable=retryable;
}
static void operation_begin(void){
    memset(&operation,0,sizeof(operation));memset(&session,0,sizeof(session));
    osg_copy(operation.instance,sizeof(operation.instance),assigned);osg_copy(operation.publication,sizeof(operation.publication),publication);
    operation.attempt=operation_attempt<UINT32_MAX?++operation_attempt:operation_attempt;operation.phase="opening";
    report(phases[state]);
}
static void disconnected(void){xSemaphoreTake(ui.mutex,portMAX_DELAY);osg_input_cancel(&input,OSG_INPUT_CANCELLED);osg_connected(ui.runtime,false);*ui.dirty=true;xSemaphoreGive(ui.mutex);}
static void stop(void);
static void abort_connection(void){osg_socket_close(&connection);disconnected();state=OFF;detail=NULL;remote_armed=false;input_sequence=0;mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));}
static const char *failure_message(const char *reason){
    if(!strcmp(reason,"wifi-connection-failed"))return "Wi-Fi connection failed";
    if(!strcmp(reason,"host-tls-or-connection-failed"))return "Could not connect to Cockpit";
    if(!strcmp(reason,"pairing-offer-expired")||!strcmp(reason,"pairing-approval-expired"))return "Pairing timed out";
    if(!strcmp(reason,"baseline-not-confirmed-or-touch-changed-reconnect-required"))return "Input activation failed";
    if(!strcmp(reason,"input-cancelled-or-lease-expired-reconnect-required"))return "Input safety check failed";
    if(!strcmp(reason,"input-backlog-or-counter-invalid-reconnect-required"))return "Input queue failed";
    if(!strcmp(reason,"session-counter-exhausted"))return "Session counter exhausted";
    if(!strncmp(reason,"trusted-clock",13))return "Device clock or setup unavailable";
    if(!strcmp(reason,"setup-storage-unavailable"))return "Setup storage unavailable";
    if(!strcmp(reason,"setup-save-failed-repair-required"))return "Could not save setup";
    if(!strcmp(reason,"setup-delete-failed"))return "Could not delete setup";
    if(!strcmp(reason,"invalid-installation-id"))return "Invalid companion identity";
    return "Unrecognized device failure";
}
static void fail(const char *reason){abort_connection();state=FAULT;detail=failure_message(reason);operation_failed(reason,true);mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));report(reason);}
static cJSON *message(const char *type){cJSON *v=cJSON_CreateObject();if(v)cJSON_AddStringToObject(v,"type",type);return v;}
static const char *request_operation(const char *type){
    if(!type)return "Request";
    if(!strcmp(type,"auth"))return "Authentication";
    if(!strcmp(type,"open"))return "Open panel";
    if(!strcmp(type,"image-chunk"))return "Image download";
    if(!strcmp(type,"heartbeat"))return "Heartbeat";
    if(!strcmp(type,"baseline"))return "Activate inputs";
    if(!strcmp(type,"input"))return "Send input";
    if(!strcmp(type,"pair-status"))return "Pairing approval";
    if(!strcmp(type,"pair"))return "Pairing";
    return "Close panel";
}
static bool request(cJSON *v,cJSON **reply,unsigned timeout){
    request_label=request_operation(osg_string(v,"type"));
    connection.request_error=NULL;
    const bool ok=osg_socket_request(&connection,v,reply,timeout);
    const char *reason=osg_string(*reply,"error");
    if(!reason||!*reason)reason=connection.request_error;
    if(!reason)reason=ok?"Invalid reply":"No valid response received";
    snprintf(request_detail,sizeof(request_detail),"%s: %s",request_label,reason);
    /* Keep diagnostics bounded and printable; never copy the request or credentials. */
    for(char *p=request_detail;*p;p++)if((unsigned char)*p<32||(unsigned char)*p>126)*p=' ';
    return ok;
}
static void fail_request(void){
    osg_copy(fault_detail,sizeof(fault_detail),request_detail);
    abort_connection();state=FAULT;detail=fault_detail;operation_failed("request-failed",true);report("cockpit-request-failed");
}
static bool request_problem(const char *reason){snprintf(request_detail,sizeof(request_detail),"%s: %s",request_label,reason);return false;}
static bool equal(const char *a,const char *b){return a&&b&&!strcmp(a,b);}
static bool identity(const cJSON *reply,const char *type){
    if(!equal(osg_string(reply,"type"),type))return request_problem("Unexpected reply type");
    if(!equal(osg_string(reply,"hostId"),settings.host))return request_problem("Reply belongs to another host");
    if(!equal(osg_string(reply,"companionId"),settings.companion))return request_problem("Reply belongs to another companion");
    return true;
}
static cJSON *session_message(const char *type,const char *sequence){
    cJSON *v=message(type);
    cJSON_AddNumberToObject(v,"version",1);cJSON_AddStringToObject(v,"clientId",session.client);
    cJSON_AddStringToObject(v,"sessionId",session.session);cJSON_AddStringToObject(v,"generation",session.generation);
    cJSON_AddStringToObject(v,"publicationHash",session.publication);
    if(sequence)cJSON_AddStringToObject(v,"sequence",sequence);
    if(state==PANEL_ERROR&&!strcmp(type,"heartbeat"))cJSON_AddBoolToObject(v,"panelError",true);
    if(!strcmp(type,"heartbeat")||!strcmp(type,"image-chunk")||!strcmp(type,"baseline"))add_operation(v);
    return v;
}
static bool image_failure(const char *code,const char *reason,bool retryable){osg_copy(fault_detail,sizeof(fault_detail),reason);detail=fault_detail;operation_failed(code,retryable);osg_pixels_panel_release(loading_owner);return false;}
static bool cache_failure(void){
    const char *code=osg_pixels_error();
    if(equal(code,"cache_full"))return image_failure(code,"Image cache is full while the displayed panel is protected; reduce image sizes or reuse images",false);
    if(equal(code,"cache_unavailable"))return image_failure(code,"Image cache storage is unavailable",false);
    if(equal(code,"cache_active_corrupt"))return image_failure(code,"A displayed image is damaged; restart the companion to reload it safely",false);
    if(equal(code,"cache_corrupt"))return image_failure(code,"Saved image verification failed",true);
    if(equal(code,"cache_invalid"))return image_failure(code,"Panel image cache request is invalid",false);
    return image_failure(code,"Could not read or write image cache storage",true);
}
static bool pending_mutation(void){
    if(!interrupted_command)interrupted_command=osg_serial_take_command();
    /* A command can arrive during synchronous flash work, outside any serial
     * request. Drain complete records without executing mutations on this stack. */
    capture_command=true;
    int c;size_t read=0;
    while(!interrupted_command&&read++<OSG_PROVISION_BYTES+2u&&(c=getchar())!=EOF)serial_byte(c);
    clearerr(stdin);capture_command=false;
    return interrupted_command!=NULL;
}
static bool image_cancelled(void){
    if(!pending_mutation())return false;
    load_interrupted=true;image_failure("cancelled","Panel loading cancelled",true);return true;
}
static bool panel_error_ack(const cJSON *,const char *);
static bool preparation_heartbeat(void){
    if(image_cancelled())return false;
    if(session.heartbeat>=UINT64_MAX-1)return image_failure("session-counter-exhausted","Session counter exhausted",true);
    char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++session.heartbeat);
    cJSON *reply=NULL;
    bool received=request(session_message("heartbeat",sequence),&reply,700);
    bool ok=received&&panel_error_ack(reply,sequence);cJSON_Delete(reply);
    if(!ok)return image_failure("preparation-heartbeat-failed",received?"Cockpit rejected panel preparation heartbeat":request_detail,true);
    return !image_cancelled();
}
static bool load_images(osg_image *image,unsigned slot){
    loading_owner=slot;load_interrupted=false;detail="Checking saved images";operation_phase("checking-cache");update_screen();
    size_t total=0;
    unsigned assets[OSG_MAX_WIDGETS]={0};bool acquired[OSG_MAX_WIDGETS]={false},cached[OSG_MAX_WIDGETS]={false};
    /* The caller has fenced the inactive panel against the renderer. Keep the
     * displayed owner's pins until it is itself safely reusable on a later switch. */
    if(!osg_pixels_panel_begin(slot))return cache_failure();
    for(size_t i=0;i<image->panel.widget_count;i++){
        osg_widget *w=&image->widgets[i];if(w->kind!=OSG_IMAGE)continue;
        int x,y,width,height;osg_panel_widget_rect(&image->panel,i,ui.runtime->display.width,ui.runtime->display.height,&x,&y,&width,&height);
        if(width<1||height<1||width>800||height>800)return image_failure("image-dimensions","Invalid image dimensions",false);
        if(!osg_hash_id(image->image_hashes[i]))return image_failure("image-identity","Invalid image content identity",false);
        w->image_width=(uint16_t)width;w->image_height=(uint16_t)height;
        bool alias=false;
        for(size_t before=0;before<i;before++)if(image->widgets[before].kind==OSG_IMAGE&&
            image->widgets[before].image_width==w->image_width&&image->widgets[before].image_height==w->image_height&&
            equal(image->image_hashes[before],image->image_hashes[i])){alias=true;break;}
        if(!alias)total+=(size_t)width*height*3;
        if(total>1200*1024)return image_failure("image-budget","Panel images exceed storage limit",false);
    }
    operation.total=total;operation.completed=0;operation.hits=operation.misses=0;
    if(!total)return !image_cancelled();
    /* Tell the host this exact unarmed session is preparing before flash erase
     * or CRC work can block. Input leases remain strict after baseline admission. */
    if(!preparation_heartbeat())return false;
    /* Pin every existing candidate asset before reserving misses, so allocation
     * cannot evict a reusable image that occurs later in the new panel. */
    for(unsigned pass=0;pass<2;pass++)for(size_t i=0;i<image->panel.widget_count;i++){
        osg_widget *w=&image->widgets[i];if(w->kind!=OSG_IMAGE||acquired[i])continue;
        const size_t bytes=(size_t)w->image_width*w->image_height*3;
        for(size_t before=0;before<i;before++)if(acquired[before]&&image->widgets[before].kind==OSG_IMAGE&&
            image->widgets[before].image_width==w->image_width&&image->widgets[before].image_height==w->image_height&&
            equal(image->image_hashes[before],image->image_hashes[i])){
            assets[i]=assets[before];cached[i]=cached[before];acquired[i]=true;break;
        }
        if(acquired[i])continue;
        if(!pass&&!osg_pixels_contains(image->image_hashes[i],w->image_width,w->image_height,bytes))continue;
        if(image_cancelled())return false;
        if(pass&&!preparation_heartbeat())return false;
        if(!osg_pixels_acquire(slot,image->image_hashes[i],w->image_width,w->image_height,bytes,&assets[i],&cached[i]))
            return cache_failure();
        acquired[i]=true;
        if(cached[i]){operation.hits++;operation.completed+=bytes;}else operation.misses++;
    }
    static unsigned char chunk[6144];
    for(size_t i=0;i<image->panel.widget_count;i++){
        osg_widget *w=&image->widgets[i];if(w->kind!=OSG_IMAGE)continue;
        const size_t bytes=(size_t)w->image_width*w->image_height*3;
        /* The same image may occur more than once in a panel. Once its first
         * widget finishes, later aliases map that entry without another transfer. */
        if(!cached[i])for(size_t before=0;before<i;before++)if(image->widgets[before].kind==OSG_IMAGE&&assets[before]==assets[i]){
            cached[i]=true;break;
        }
        for(size_t offset=0;!cached[i]&&offset<bytes;){
            if(image_cancelled())return false;
            detail="Downloading missing images";operation_phase("downloading");
            cJSON *v=session_message("image-chunk",NULL),*reply=NULL;
            cJSON_AddStringToObject(v,"widgetId",w->id);cJSON_AddNumberToObject(v,"width",w->image_width);cJSON_AddNumberToObject(v,"height",w->image_height);cJSON_AddNumberToObject(v,"offset",(double)offset);
            bool received=request(v,&reply,3000);
            bool ok=received&&cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(reply,"ok"))&&equal(osg_string(reply,"type"),"image-chunk")&&equal(osg_string(reply,"widgetId"),w->id)&&equal(osg_string(reply,"publicationHash"),session.publication);
            size_t count=0;const char *encoded=osg_string(reply,"base64");
            ok=ok && cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"offset"))==(double)offset && cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"total"))==(double)bytes && cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"width"))==w->image_width && cJSON_GetNumberValue(cJSON_GetObjectItemCaseSensitive(reply,"height"))==w->image_height && encoded && osg_platform_crypto.base64(encoded,chunk,sizeof(chunk),&count) && count && count<=bytes-offset && count%3==0;
            cJSON_Delete(reply);if(!ok)return image_failure(received?"image-download-invalid":"image-download-failed",received?"Image download: Invalid image data":request_detail,!received);
            if(image_cancelled())return false;
            if(!osg_pixels_write(assets[i],offset,chunk,count))return cache_failure();
            offset+=count;operation.completed+=count;
            static char progress[64];snprintf(progress,sizeof(progress),"Loading images %u%%",(unsigned)(operation.completed*100/total));detail=progress;update_screen();
            if(millis()-operation_reported>=500)report(phases[state]);
        }
        w->pixels=osg_pixels_finish(assets[i]);if(!w->pixels)return cache_failure();
    }
    return !image_cancelled();
}
static void stop(void){
    const bool close=(state==LIVE||state==PANEL_ERROR)&&(connection.ws||connection.direct);
    disconnected(); /* Stop local input before any potentially blocking I/O. */
    if(close){cJSON *reply=NULL;(void)request(session_message("close",NULL),&reply,300);cJSON_Delete(reply);}
    abort_connection();
    memset(&operation,0,sizeof(operation));memset(&session,0,sizeof(session));detail=NULL;
}
static bool panel_error_ack(const cJSON *v,const char *sequence){
    return cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(v,"ok"))&&equal(osg_string(v,"type"),"ack")&&
        equal(osg_string(v,"clientId"),session.client)&&equal(osg_string(v,"sessionId"),session.session)&&
        equal(osg_string(v,"generation"),session.generation)&&equal(osg_string(v,"publicationHash"),session.publication)&&
        equal(osg_string(v,"sequence"),sequence)&&equal(osg_string(v,"ackStage"),"source-accepted")&&cJSON_IsFalse(cJSON_GetObjectItemCaseSensitive(v,"submitted"));
}
static bool acknowledgement(cJSON *reply,const char *sequence,uint64_t started){
    if(!equal(osg_string(reply,"type"),"ack"))return request_problem("Unexpected acknowledgement type");
    if(!equal(osg_string(reply,"clientId"),session.client)||!equal(osg_string(reply,"sessionId"),session.session))return request_problem("Reply belongs to another session");
    if(!equal(osg_string(reply,"generation"),session.generation)||!equal(osg_string(reply,"publicationHash"),session.publication))return request_problem("Reply uses another panel revision");
    if(!equal(osg_string(reply,"sequence"),sequence))return request_problem("Reply sequence does not match request");
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const uint64_t before=session.feedback_sequence,before_generation=session.binding_generation;const bool seen=session.feedback_seen;
    bool ok=osg_input_monitor(&input,millis())&&osg_ack(reply,&session,sequence,ui.runtime,millis());
    if(ok){input.last_ack_ms=started;if(!seen||before!=session.feedback_sequence||before_generation!=session.binding_generation)*ui.dirty=true;}
    xSemaphoreGive(ui.mutex);return ok?true:request_problem("Input lease or acknowledgement invalid");
}
void osg_network_status(const char *request_id){
    cJSON *v=cJSON_CreateObject();char encoded[2048],epoch[21];
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const char *status=state==LIVE&&input.mode==OSG_INPUT_ARMED?"live-armed":phases[state];
    cJSON_AddStringToObject(v,"status",status);
    if(detail)cJSON_AddStringToObject(v,"statusMessage",detail);
    if(osg_stable_id(request_id))cJSON_AddStringToObject(v,"requestId",request_id);
    cJSON_AddNumberToObject(v,"protocolVersion",2);
    cJSON_AddStringToObject(v,"deviceType","esp32");
    cJSON_AddStringToObject(v,"firmwareVersion",OSG_FIRMWARE_VERSION);
    cJSON_AddStringToObject(v,"boardProfile",OSG_BOARD_PROFILE);
    cJSON_AddStringToObject(v,"companionId",settings.companion);
    cJSON_AddBoolToObject(v,"touchAvailable",ui.touch_capable);
    cJSON_AddBoolToObject(v,"setupSaved",installation_saved);
    cJSON_AddStringToObject(v,"connectionMode",settings.direct?"serial":"wifi");
    cJSON_AddStringToObject(v,"name",settings.name);
    cJSON_AddStringToObject(v,"ssid",settings.ssid);
    cJSON_AddBoolToObject(v,"wifiPasswordSaved",settings.password[0]!=0);
    cJSON_AddBoolToObject(v,"readyForArm",state==LIVE&&!remote_armed&&osg_input_can_arm(&input,millis()));
    if((state==LIVE||state==OPEN||state==PANEL_ERROR||state==FAULT)&&session.session[0]){
        snprintf(epoch,sizeof(epoch),"%" PRIu64,input.epoch);
        cJSON_AddStringToObject(v,"sessionId",session.session);cJSON_AddStringToObject(v,"generation",session.generation);
        cJSON_AddStringToObject(v,"publicationHash",session.publication);cJSON_AddStringToObject(v,"armEpoch",epoch);
    }
    add_operation(v);
    xSemaphoreGive(ui.mutex);
    if(v&&cJSON_PrintPreallocated(v,encoded,sizeof(encoded),false)){puts(encoded);fflush(stdout);}cJSON_Delete(v);
}
static void arm(const cJSON *command){
    uint64_t challenge,epoch=0;
    if(state!=LIVE||remote_armed||!equal(osg_string(command,"sessionId"),session.session)||
        !equal(osg_string(command,"generation"),session.generation)||!equal(osg_string(command,"publicationHash"),session.publication)||
        !osg_counter(osg_string(command,"armEpoch"),&challenge)){report("arm-rejected-stale-or-missing-session");return;}
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    bool matched=challenge==input.epoch;
    bool prepared=matched&&osg_input_prepare(&input,millis(),&epoch);
    if(matched&&!prepared&&input.epoch<UINT64_MAX)input.epoch++; /* Consume rejected tickets too. */
    if(prepared)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
    if(!prepared){report("arm-rejected-release-touch-check-display-and-leases");return;}
    operation_phase("awaiting-neutral");report("arming");
    cJSON *v=session_message("baseline","0"),*values=cJSON_AddObjectToObject(v,"values"),*reply=NULL;
    for(size_t i=0;i<ui.runtime->panel->widget_count;i++){
        const osg_widget *w=&ui.runtime->panel->widgets[i];osg_value_kind kind;double fallback;
        if(osg_widget_input(w,&kind,&fallback)){if(kind==OSG_BOOL)cJSON_AddBoolToObject(values,w->reference,false);else cJSON_AddNumberToObject(values,w->reference,fallback);}
    }
    /* The host may accept even if the ACK is lost. Every failure below closes
     * this connection; this baseline is never retried or silently replayed. */
    const uint64_t started=millis();remote_armed=true;
    bool received=request(v,&reply,700);
    bool ok=received&&acknowledgement(reply,"0",started);
    cJSON_Delete(reply);
    if(pending_mutation()){
        abort_connection();state=OFF;detail="Input activation cancelled";operation_failed("cancelled",true);report("panel-load-cancelled");return;
    }
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(ok)ok=osg_input_commit(&input,epoch,started,millis());
    *ui.dirty=true;xSemaphoreGive(ui.mutex);
    if(!ok){if(!received)fail_request();else fail("baseline-not-confirmed-or-touch-changed-reconnect-required");return;}
    input_sequence=0;next_input_request=next_request=millis();detail=NULL;operation_phase("ready");report("live-armed");
}
/* Connection alone is insufficient until the rendered panel and released touch
 * are healthy. Reuse the same session/epoch-bound neutral-baseline handshake. */
static void arm_when_ready(void){
    if(state!=LIVE||remote_armed)return;
    if(pending_mutation())return;
    char epoch[21];cJSON *ticket=NULL;
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const bool display_ready=input.display_ready;
    const bool touch_capable=input.touch_capable;
    const bool touch_valid=input.touch_valid;
    if(osg_input_can_arm(&input,millis())){
        ticket=cJSON_CreateObject();
        snprintf(epoch,sizeof(epoch),"%" PRIu64,input.epoch);
        cJSON_AddStringToObject(ticket,"sessionId",session.session);
        cJSON_AddStringToObject(ticket,"generation",session.generation);
        cJSON_AddStringToObject(ticket,"publicationHash",session.publication);
        cJSON_AddStringToObject(ticket,"armEpoch",epoch);
    }
    xSemaphoreGive(ui.mutex);
    if(!touch_capable){
        if(!equal(operation.error,"touch-unavailable")){detail="Touch input is unavailable";operation_failed("touch-unavailable",false);report("panel-input-unavailable");}
        return;
    }
    if(!display_ready){detail="Rendering panel";operation_phase("rendering");}
    else {detail=touch_valid?"Release touch to activate inputs":"Waiting for healthy touch input";operation_phase("awaiting-neutral");}
    if(ticket){arm(ticket);cJSON_Delete(ticket);}
}
static bool choose(const cJSON *reply){
    const cJSON *list=cJSON_GetObjectItemCaseSensitive(reply,"assignments");
    if(!cJSON_IsArray(list)||cJSON_GetArraySize(list)>8)return false;
    const cJSON *selected=NULL;
    for(const cJSON *v=list->child;v;v=v->next){
        if(!osg_stable_id(osg_string(v,"instanceId"))||!osg_hash_id(osg_string(v,"publicationHash")))return false;
    }
    if(cJSON_GetArraySize(list)==1)selected=list->child;
    if(!selected){memset(&operation,0,sizeof(operation));memset(&session,0,sizeof(session));detail=NULL;state=WAIT_ASSIGNMENT;next_request=millis()+2000;report("choose-or-assign-one-instance-in-cockpit");return true;}
    if(!osg_copy(assigned,sizeof(assigned),osg_string(selected,"instanceId"))||!osg_copy(publication,sizeof(publication),osg_string(selected,"publicationHash")))return false;
    state=OPEN;detail="Opening assigned panel";operation_begin();return true;
}
static bool open_socket(void){return connection.ws||connection.direct||osg_socket_open(&connection,&settings);}
static void advance(void){
    if(state==OFF||state==FAULT)return;
    bool send_input=false;
    if(state==LIVE){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        bool healthy=osg_input_monitor(&input,millis());
        send_input=remote_armed&&input.mode==OSG_INPUT_ARMED&&input.count&&millis()>=next_input_request;
        osg_input_fault fault=input.fault;
        bool render_failed=panel_render_failed;
        xSemaphoreGive(ui.mutex);
        if(!healthy||render_failed){
            if(render_failed||fault==OSG_INPUT_DISPLAY_FAILED){disconnected();state=PANEL_ERROR;detail="Panel could not render correctly";operation_failed("render-failed",true);next_request=millis();report("panel-render-failed");return;}
            fail("input-cancelled-or-lease-expired-reconnect-required");
            if(fault==OSG_INPUT_TOUCH_FAILED)detail="Touch input failed";
            else if(fault==OSG_INPUT_UI_EXPIRED)detail="Touch update timed out";
            else if(fault==OSG_INPUT_HOST_EXPIRED)detail="Cockpit response timed out";
            return;
        }
    }
    if(millis()<next_request&&!send_input)return;
    cJSON *reply=NULL,*v=NULL;bool ok=false;
    if(state==PAIR_BEGIN){
        if(millis()>=pair_deadline){fail("pairing-offer-expired");return;}
        if(!open_socket()){fail("host-tls-or-connection-failed");return;}
        v=message("pair");cJSON_AddStringToObject(v,"secret",settings.secret);cJSON_AddStringToObject(v,"companionId",settings.companion);cJSON_AddStringToObject(v,"name",settings.name);
        ok=request(v,&reply,3000)&&equal(osg_string(reply,"type"),"pair-pending")&&osg_stable_id(osg_string(reply,"requestId"))&&osg_copy(pending,sizeof(pending),osg_string(reply,"requestId"));
        if(ok){state=PAIR_POLL;next_request=millis()+400;report("approve-pairing-in-cockpit");}
    }else if(state==PAIR_POLL){
        if(millis()>=pair_deadline){fail("pairing-approval-expired");return;}
        v=message("pair-status");cJSON_AddStringToObject(v,"requestId",pending);cJSON_AddStringToObject(v,"secret",settings.secret);
        ok=request(v,&reply,3000);
        if(ok&&equal(osg_string(reply,"type"),"paired")){
            ok=identity(reply,"paired")&&osg_secret(osg_string(reply,"token"))&&osg_copy(settings.token,sizeof(settings.token),osg_string(reply,"token"));
            if(ok){mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));
                if(settings.persist&&!osg_store_save(&settings)){cJSON_Delete(reply);fail("setup-save-failed-repair-required");return;}
                installation_saved=settings.persist;state=AUTH;report("paired");}
        }else if(ok){ok=equal(osg_string(reply,"type"),"pair-pending")&&equal(osg_string(reply,"requestId"),pending);next_request=millis()+400;}
    }else if(state==AUTH||state==WAIT_ASSIGNMENT){
        /* Assignment refresh requires a fresh authenticated connection. Keep
         * the confirmed idle screen until a panel arrives or this probe fails;
         * routine polling is not a visible reconnect operation. */
        if(state==WAIT_ASSIGNMENT)osg_socket_close(&connection);
        if(!open_socket()){fail("host-tls-or-connection-failed");return;}
        v=message("auth");cJSON_AddStringToObject(v,"token",settings.token);
        ok=request(v,&reply,3000)&&identity(reply,"authenticated")&&choose(reply);
    }else if(state==OPEN){
        load_interrupted=false;detail="Opening assigned panel";update_screen();
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        bool busy=*ui.drawing_panel==&images[1-image_index].panel;
        xSemaphoreGive(ui.mutex);if(busy)return;
        v=message("open");cJSON_AddStringToObject(v,"instanceId",assigned);cJSON_AddStringToObject(v,"publicationHash",publication);cJSON_AddBoolToObject(v,"takeover",false);cJSON_AddBoolToObject(v,"externalImages",true);
        ok=request(v,&reply,3000)&&identity(reply,"session-open");
        if(ok&&!equal(osg_string(reply,"controllerInstanceId"),assigned))ok=request_problem("Reply belongs to another panel");
        if(ok&&!equal(osg_string(reply,"publicationHash"),publication))ok=request_problem("Reply uses another panel revision");
        if(ok&&!osg_open_identity(reply,settings.host,settings.companion,assigned,publication,&session))ok=request_problem("Invalid session identifiers");
        if(ok){
            osg_copy(operation.session,sizeof(operation.session),session.session);osg_copy(operation.generation,sizeof(operation.generation),session.generation);
            osg_image *candidate=&images[1-image_index];
            const cJSON *p=cJSON_GetObjectItemCaseSensitive(reply,"publication");
            ok=osg_decode_publication(p,publication,osg_platform_crypto,candidate,ui.runtime->display.width,ui.runtime->display.height);
            cJSON_Delete(reply);reply=NULL;
            if(ok)ok=load_images(candidate,1-image_index);
            else {detail="Panel data invalid or unsupported";operation_failed("invalid-panel",false);osg_pixels_panel_release(1-image_index);}
            if(!ok){
                if(load_interrupted){abort_connection();state=OFF;report("panel-load-cancelled");update_screen();return;}
                disconnected();state=PANEL_ERROR;next_request=millis();report("panel-load-failed");update_screen();return;
            }
            if(ok){xSemaphoreTake(ui.mutex,portMAX_DELAY);
                ok=osg_init(ui.runtime,&candidate->panel,ui.runtime->display,NULL,NULL);
                if(ok){image_index=1-image_index;panel_render_failed=false;osg_connected(ui.runtime,true);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());remote_armed=false;input_sequence=0;*ui.dirty=true;}
                xSemaphoreGive(ui.mutex);}
            if(ok){detail="Rendering panel";state=LIVE;next_request=millis();operation_phase("rendering");report("live-disarmed");}
            else {osg_pixels_panel_release(1-image_index);disconnected();state=PANEL_ERROR;detail="Panel layout is invalid";operation_failed("invalid-layout",false);next_request=millis();report("panel-load-failed");return;}
        }
    }else if(state==LIVE||state==PANEL_ERROR){
        if(millis()>=next_request){
            if(session.heartbeat>=UINT64_MAX-1){fail("session-counter-exhausted");return;}
            char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++session.heartbeat);
            const uint64_t started=millis();
            ok=request(session_message("heartbeat",sequence),&reply,700)&&(state==PANEL_ERROR?panel_error_ack(reply,sequence):acknowledgement(reply,sequence,started));
            next_request=millis()+250;
        }else if(send_input){
            osg_input_edge edges[OSG_INPUT_BATCH_EDGES];size_t count=0;uint64_t epoch=0;
            xSemaphoreTake(ui.mutex,portMAX_DELAY);
            ok=osg_input_peek(&input,edges,OSG_INPUT_BATCH_EDGES,&count,&epoch,millis());
            xSemaphoreGive(ui.mutex);
            if(!ok||!count||input_sequence>=UINT64_MAX-1){fail("input-backlog-or-counter-invalid-reconnect-required");return;}
            char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++input_sequence);
            v=session_message("input",sequence);cJSON *changes=cJSON_AddArrayToObject(v,"changes");
            for(size_t i=0;i<count;i++){
                cJSON *change=cJSON_CreateObject();cJSON_AddStringToObject(change,"controlId",ui.runtime->panel->widgets[edges[i].widget].reference);
                if(edges[i].kind==OSG_BOOL)cJSON_AddBoolToObject(change,"value",edges[i].down);else cJSON_AddNumberToObject(change,"value",edges[i].number);cJSON_AddItemToArray(changes,change);
            }
            const uint64_t started=millis();
            ok=request(v,&reply,400)&&acknowledgement(reply,sequence,started);
            xSemaphoreTake(ui.mutex,portMAX_DELAY);
            if(ok)ok=osg_input_accept(&input,epoch,count);
            xSemaphoreGive(ui.mutex);next_input_request=millis()+OSG_INPUT_BATCH_INTERVAL_MS;
        }
    }
    /* Cockpit invalidates the old session when it changes assignment.
     * Reauthenticate for the current assignment, cancelling all old input first. */
    bool assignment_refresh=!ok&&(state==LIVE||state==PANEL_ERROR)&&equal(osg_string(reply,"type"),"error")&&
        equal(osg_string(reply,"error"),"Session is obsolete; explicitly reconnect.");
    cJSON_Delete(reply);
    if(assignment_refresh){abort_connection();state=AUTH;detail="Assignment changed; reconnecting safely";operation_failed("cancelled",true);next_request=millis()+500;report("refreshing-cockpit-assignment-disarmed");}
    else if(!ok)fail_request();
}
static bool clock_from(const cJSON *command){
    const cJSON *value=cJSON_GetObjectItemCaseSensitive(command,"unixMs");
    if(!cJSON_IsNumber(value)||value->valuedouble<1735689600000.0||value->valuedouble>4102444800000.0||floor(value->valuedouble)!=value->valuedouble)return false;
    uint64_t ms=(uint64_t)value->valuedouble;const struct timeval tv={.tv_sec=(time_t)(ms/1000),.tv_usec=(suseconds_t)((ms%1000)*1000)};
    return settimeofday(&tv,NULL)==0;
}
static void install_identity(void){
    unsigned char id[16];esp_fill_random(id,sizeof(id));id[6]=(id[6]&15)|64;id[8]=(id[8]&63)|128;
    snprintf(settings.companion,sizeof(settings.companion),"%02x%02x%02x%02x-%02x%02x-%02x%02x-%02x%02x-%02x%02x%02x%02x%02x%02x",id[0],id[1],id[2],id[3],id[4],id[5],id[6],id[7],id[8],id[9],id[10],id[11],id[12],id[13],id[14],id[15]);
}
static void command(cJSON *v){
    const char *type=osg_string(v,"command");
    if(equal(type,"status")){osg_network_status(osg_string(v,"requestId"));return;}
    if(equal(type,"scan-wifi")){
        stop();osg_wifi_stop();cJSON *list=osg_wifi_scan(),*reply=cJSON_CreateObject();
        cJSON_AddStringToObject(reply,"status",list?"wifi-networks":"wifi-scan-failed");
        if(list)cJSON_AddItemToObject(reply,"networks",list);
        char encoded[4096];if(cJSON_PrintPreallocated(reply,encoded,sizeof(encoded),false)){puts(encoded);fflush(stdout);}cJSON_Delete(reply);return;
    }
    if(equal(type,"arm")){arm(v);return;}
    if(equal(type,"disarm")){stop();report("disarmed-reconnect-required");return;}
    if(equal(type,"stop")){stop();report("stopped");return;}
    if(equal(type,"page")){
        const cJSON *page=cJSON_GetObjectItemCaseSensitive(v,"page");bool ok=false;
        if(cJSON_IsNumber(page)&&page->valuedouble>=0&&page->valuedouble<ui.runtime->panel->page_count&&floor(page->valuedouble)==page->valuedouble){
            if(remote_armed)stop();
            xSemaphoreTake(ui.mutex,portMAX_DELAY);ok=osg_set_page(ui.runtime,(size_t)page->valuedouble);
            if(ok){if(state==LIVE)osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());*ui.dirty=true;}
            xSemaphoreGive(ui.mutex);
        }
        report(ok?"page-changed":"invalid-page");return;
    }
    if(equal(type,"forget")){stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));bool forgotten=osg_store_forget();if(forgotten)installation_saved=false;report(forgotten?"forgotten":"setup-delete-failed");return;}
    if(equal(type,"connect")){
        const char *instance=osg_string(v,"instanceId");
        if(!osg_secret(settings.token)||(instance&&*instance&&!osg_stable_id(instance))){report("pair-first-or-invalid-instance");return;}
        stop();/* Panel selection is resolved exclusively from Cockpit assignments. */
        if(!settings.direct&&!osg_wifi_start(&settings)){fail("wifi-connection-failed");return;}
        if(cJSON_GetObjectItemCaseSensitive(v,"unixMs")&&!clock_from(v)){fail("trusted-clock-invalid");return;}
        state=AUTH;next_request=millis();return;
    }
    if(equal(type,"provision-direct")){
        const char *host=osg_string(v,"hostId"),*secret=osg_string(v,"secret"),*id=osg_string(v,"companionId"),*name=osg_string(v,"name");
        if(!osg_stable_id(host)||!osg_stable_id(id)||!osg_secret(secret)||!name||!*name||strlen(name)>80){report("invalid-provisioning-record");return;}
        stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));
        installation_saved=false;
        if(!osg_store_available()){fail("setup-storage-unavailable");return;}
        if(!osg_store_forget()){fail("setup-delete-failed");return;}
        settings.schema=2;settings.persist=true;settings.direct=true;
        osg_copy(settings.host,sizeof(settings.host),host);osg_copy(settings.secret,sizeof(settings.secret),secret);
        osg_copy(settings.companion,sizeof(settings.companion),id);osg_copy(settings.name,sizeof(settings.name),name);
        if(!osg_store_available()){fail("setup-storage-unavailable");return;}
        pair_deadline=millis()+60000;state=PAIR_BEGIN;next_request=millis();report("pairing");return;
    }
    if(equal(type,"provision")){
        osg_settings candidate;
        if(!osg_settings_parse(v,&candidate)){mbedtls_platform_zeroize(&candidate,sizeof(candidate));report("invalid-provisioning-record");return;}
        if(cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(v,"keepWifiPassword"))){
            if(!settings.password[0] || strcmp(settings.ssid,candidate.ssid)){
                mbedtls_platform_zeroize(&candidate,sizeof(candidate));report("saved-wifi-password-unavailable");return;
            }
            memcpy(candidate.password,settings.password,sizeof(candidate.password));
        }
        if(candidate.persist&&!osg_store_available()){mbedtls_platform_zeroize(&candidate,sizeof(candidate));report("setup-storage-unavailable");return;}
        stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));installation_saved=false;
        /* Commit the deletion before starting a new handshake: a failed pairing
         * or reboot must never restore the previous installation. */
        if(osg_store_available()&&!osg_store_forget()){
            mbedtls_platform_zeroize(&candidate,sizeof(candidate));fail("setup-delete-failed");return;
        }
        settings=candidate;mbedtls_platform_zeroize(&candidate,sizeof(candidate));
        if(!clock_from(v)){fail("trusted-clock-invalid");return;}
        install_identity();
        const char *id=osg_string(v,"companionId");
        if(id){if(!osg_stable_id(id)){fail("invalid-installation-id");return;}osg_copy(settings.companion,sizeof(settings.companion),id);}
        pair_deadline=millis()+60000;
    }else if(equal(type,"resume")){
        stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));
        if(!clock_from(v)||!osg_store_load(&settings)){fail("trusted-clock-or-saved-installation-unavailable");return;}
        installation_saved=true;
    }else{report("unsupported-command");return;}
    if(!settings.direct&&!osg_wifi_start(&settings)){osg_wifi_stop();fail("wifi-connection-failed");return;}
    state=settings.token[0]?AUTH:PAIR_BEGIN;next_request=millis();report(phases[state]);
}
static bool command_mutates(const char *type){
    return equal(type,"stop")||equal(type,"disarm")||equal(type,"page")||equal(type,"forget")||equal(type,"connect")||
        equal(type,"provision-direct")||equal(type,"provision")||equal(type,"resume")||equal(type,"scan-wifi")||
        (equal(type,"arm")&&state==LIVE&&!remote_armed);
}
static void serial_byte(int c){
    cJSON *v=NULL;
    osg_serial_record record=osg_serial_json_byte(c,&v);
    if(record==OSG_SERIAL_TOO_LARGE)report("provisioning-line-too-large");
    else if(record==OSG_SERIAL_INVALID_JSON)report("invalid-command-json");
    else if(v){
        const char *type=osg_string(v,"command");
        /* A late transport reply can finish after its request timed out. It is
         * never a console command or authority to cancel current panel work. */
        if(equal(type,"serial-reply"))cJSON_Delete(v);
        else if(capture_command&&command_mutates(type))interrupted_command=v;
        else {command(v);cJSON_Delete(v);}
    }
}
static void update_screen(void){
    osg_screen_state screen=state==FAULT ? OSG_SCREEN_ERROR :
        state==PAIR_BEGIN||state==PAIR_POLL ? OSG_SCREEN_CONNECTING :
        !settings.token[0] ? OSG_SCREEN_UNPAIRED : state==OPEN ? OSG_SCREEN_LOADING : state==AUTH ? OSG_SCREEN_CONNECTING :
        state==LIVE ? OSG_SCREEN_PANEL : state==PANEL_ERROR ? OSG_SCREEN_PANEL_ERROR : state==WAIT_ASSIGNMENT ? OSG_SCREEN_NO_PANEL : OSG_SCREEN_DISCONNECTED;
    const char *message=state==PAIR_BEGIN||state==PAIR_POLL ? "Approve pairing in Cockpit" : (state==FAULT||state==PANEL_ERROR||state==OPEN) ? detail : NULL;
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(!message)message="";
    if(ui.runtime->screen!=screen||strcmp(ui.runtime->screen_message,message)){
        ui.runtime->screen=screen;osg_copy(ui.runtime->screen_message,sizeof(ui.runtime->screen_message),message);*ui.dirty=true;
    }
    xSemaphoreGive(ui.mutex);
}
static void network_task(void *arg){
    (void)arg;osg_json_init();
    if(osg_store_load(&settings)){
        installation_saved=true;
        if(settings.direct){state=AUTH;next_request=millis();}
        else if(!osg_wifi_start(&settings))fail("wifi-connection-failed");
        else if(!osg_wifi_clock())fail("trusted-clock-invalid");
        else {state=AUTH;next_request=millis();report("restoring-saved-setup");}
    }
    (void)fcntl(fileno(stdin),F_SETFL,O_NONBLOCK);report("serial-provisioning-ready");
    for(;;){
        cJSON *pending_command=interrupted_command?interrupted_command:osg_serial_take_command();interrupted_command=NULL;
        if(pending_command){command(pending_command);cJSON_Delete(pending_command);}
        int c;while((c=getchar())!=EOF)serial_byte(c);clearerr(stdin);
        update_screen();advance();update_screen();arm_when_ready();vTaskDelay(pdMS_TO_TICKS(10));
    }
}
void osg_network_touch(bool valid,bool down,int x,int y){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    bool was_armed=ui.runtime->armed,was_arming=ui.runtime->arming;int previous=ui.runtime->active;
    osg_input_poll(&input,valid,down,x,y,millis());
    if(was_armed!=ui.runtime->armed||was_arming!=ui.runtime->arming||previous!=ui.runtime->active)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
}
void osg_network_render(const osg_panel *panel,bool finished,bool success){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    osg_input_render(&input,panel,finished,success,millis());
    if(finished&&!success&&panel==ui.runtime->panel&&ui.runtime->screen==OSG_SCREEN_PANEL)panel_render_failed=true;
    if(!success)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
}
void osg_network_start(osg_network_ui value){ui=value;ui.runtime->screen=OSG_SCREEN_UNPAIRED;osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());if(!osg_serial_start()){report("serial-driver-initialization-failed");return;}if(xTaskCreate(network_task,"companion-network",24576,NULL,4,NULL)!=pdPASS)report("network-task-allocation-failed");}
