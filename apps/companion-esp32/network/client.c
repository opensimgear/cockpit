#include "platform.h"
#include "input.h"
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

typedef enum { OFF, PAIR_BEGIN, PAIR_POLL, AUTH, WAIT_ASSIGNMENT, OPEN, LIVE, FAULT } phase;
static osg_network_ui ui;
static osg_settings settings;
static osg_socket connection;
static osg_image images[2];
static unsigned image_index;
static osg_live_session session;
static osg_input input;
static bool remote_armed;
static uint64_t input_sequence, next_input_request;
static phase state;
static char pending[65], assigned[65], publication[72];
static uint64_t next_request, pair_deadline;
static char serial_line[OSG_PROVISION_BYTES+1];
static const char *phases[]={"offline","pairing","awaiting-approval","authenticating","awaiting-assignment","opening-panel","live-disarmed","error"};
static uint64_t millis(void){return (uint64_t)(esp_timer_get_time()/1000);}
static void report(const char *status){printf("{\"status\":\"%s\"}\n",status);fflush(stdout);}
static void disconnected(void){xSemaphoreTake(ui.mutex,portMAX_DELAY);osg_input_cancel(&input,OSG_INPUT_CANCELLED);osg_connected(ui.runtime,false);*ui.dirty=true;xSemaphoreGive(ui.mutex);}
static void stop(void);
static void abort_connection(void){osg_socket_close(&connection);disconnected();state=OFF;remote_armed=false;input_sequence=0;mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));}
static void fail(const char *reason){abort_connection();state=FAULT;mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));report(reason);}
static cJSON *message(const char *type){cJSON *v=cJSON_CreateObject();if(v)cJSON_AddStringToObject(v,"type",type);return v;}
static bool request(cJSON *v,cJSON **reply,unsigned timeout){return osg_socket_request(&connection,v,reply,timeout);}
static bool equal(const char *a,const char *b){return a&&b&&!strcmp(a,b);}
static bool identity(const cJSON *reply,const char *type){return equal(osg_string(reply,"type"),type)&&equal(osg_string(reply,"hostId"),settings.host)&&equal(osg_string(reply,"companionId"),settings.companion);}
static cJSON *session_message(const char *type,const char *sequence){
    cJSON *v=message(type);
    cJSON_AddNumberToObject(v,"version",1);cJSON_AddStringToObject(v,"clientId",session.client);
    cJSON_AddStringToObject(v,"sessionId",session.session);cJSON_AddStringToObject(v,"generation",session.generation);
    cJSON_AddStringToObject(v,"publicationHash",session.publication);
    if(sequence)cJSON_AddStringToObject(v,"sequence",sequence);
    return v;
}
static void stop(void){
    const bool close=state==LIVE&&connection.ws;
    disconnected(); /* Stop local input before any potentially blocking I/O. */
    if(close){cJSON *reply=NULL;(void)request(session_message("close",NULL),&reply,300);cJSON_Delete(reply);}
    abort_connection();
}
static bool acknowledgement(cJSON *reply,const char *sequence,uint64_t started){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const uint64_t before=session.feedback_sequence,before_generation=session.binding_generation;const bool seen=session.feedback_seen;
    bool ok=osg_input_monitor(&input,millis())&&osg_ack(reply,&session,sequence,ui.runtime,millis());
    if(ok){input.last_ack_ms=started;if(!seen||before!=session.feedback_sequence||before_generation!=session.binding_generation)*ui.dirty=true;}
    xSemaphoreGive(ui.mutex);return ok;
}
static void public_status(const char *request_id){
    cJSON *v=cJSON_CreateObject();char encoded[1024],epoch[21];
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const char *status=state==LIVE&&input.mode==OSG_INPUT_ARMED?"live-armed":phases[state];
    cJSON_AddStringToObject(v,"status",status);
    if(osg_stable_id(request_id))cJSON_AddStringToObject(v,"requestId",request_id);
    cJSON_AddBoolToObject(v,"touchAvailable",ui.touch_capable);
    cJSON_AddBoolToObject(v,"readyForArm",state==LIVE&&!remote_armed&&osg_input_can_arm(&input,millis()));
    if(state==LIVE){
        snprintf(epoch,sizeof(epoch),"%" PRIu64,input.epoch);
        cJSON_AddStringToObject(v,"sessionId",session.session);cJSON_AddStringToObject(v,"generation",session.generation);
        cJSON_AddStringToObject(v,"publicationHash",session.publication);cJSON_AddStringToObject(v,"armEpoch",epoch);
    }
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
    report("arming");
    cJSON *v=session_message("baseline","0"),*values=cJSON_AddObjectToObject(v,"values"),*reply=NULL;
    for(size_t i=0;i<ui.runtime->panel->widget_count;i++){
        const osg_widget *w=&ui.runtime->panel->widgets[i];if(w->kind==OSG_BUTTON)cJSON_AddBoolToObject(values,w->reference,false);
    }
    /* The host may accept even if the ACK is lost. Every failure below closes
     * this connection; this baseline is never retried or silently replayed. */
    const uint64_t started=millis();remote_armed=true;
    bool ok=request(v,&reply,700)&&acknowledgement(reply,"0",started);
    cJSON_Delete(reply);
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(ok)ok=osg_input_commit(&input,epoch,started,millis());
    *ui.dirty=true;xSemaphoreGive(ui.mutex);
    if(!ok){fail("baseline-not-confirmed-or-touch-changed-reconnect-required");return;}
    input_sequence=0;next_input_request=next_request=millis();report("live-armed");
}
static bool choose(const cJSON *reply){
    const cJSON *list=cJSON_GetObjectItemCaseSensitive(reply,"assignments");
    if(!cJSON_IsArray(list)||cJSON_GetArraySize(list)>8)return false;
    const cJSON *selected=NULL;
    for(const cJSON *v=list->child;v;v=v->next){
        if(!osg_stable_id(osg_string(v,"instanceId"))||!osg_hash_id(osg_string(v,"publicationHash")))return false;
        if(settings.instance[0]&&equal(osg_string(v,"instanceId"),settings.instance)){if(selected)return false;selected=v;}
    }
    if(!settings.instance[0]&&cJSON_GetArraySize(list)==1)selected=list->child;
    if(!selected){state=WAIT_ASSIGNMENT;next_request=millis()+2000;report("choose-or-assign-one-instance-in-cockpit");return true;}
    return osg_copy(assigned,sizeof(assigned),osg_string(selected,"instanceId"))&&osg_copy(publication,sizeof(publication),osg_string(selected,"publicationHash"))&&(state=OPEN,true);
}
static bool open_socket(void){return connection.ws||osg_socket_open(&connection,&settings);}
static void advance(void){
    if(state==OFF||state==FAULT)return;
    bool send_input=false;
    if(state==LIVE){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        bool healthy=osg_input_monitor(&input,millis());
        send_input=remote_armed&&input.mode==OSG_INPUT_ARMED&&input.count&&millis()>=next_input_request;
        xSemaphoreGive(ui.mutex);
        if(!healthy){fail("input-cancelled-or-lease-expired-reconnect-required");return;}
    }
    if(millis()<next_request&&!send_input)return;
    cJSON *reply=NULL,*v=NULL;bool ok=false;
    if(state==PAIR_BEGIN){
        if(millis()>=pair_deadline){fail("pairing-offer-expired");return;}
        if(!open_socket()){fail("host-tls-or-connection-failed");return;}
        v=message("pair");cJSON_AddStringToObject(v,"secret",settings.secret);cJSON_AddStringToObject(v,"companionId",settings.companion);cJSON_AddStringToObject(v,"name",settings.name);
        ok=request(v,&reply,1000)&&equal(osg_string(reply,"type"),"pair-pending")&&osg_stable_id(osg_string(reply,"requestId"))&&osg_copy(pending,sizeof(pending),osg_string(reply,"requestId"));
        if(ok){state=PAIR_POLL;next_request=millis()+400;report("approve-pairing-in-cockpit");}
    }else if(state==PAIR_POLL){
        if(millis()>=pair_deadline){fail("pairing-approval-expired");return;}
        v=message("pair-status");cJSON_AddStringToObject(v,"requestId",pending);cJSON_AddStringToObject(v,"secret",settings.secret);
        ok=request(v,&reply,1000);
        if(ok&&equal(osg_string(reply,"type"),"paired")){
            ok=identity(reply,"paired")&&osg_secret(osg_string(reply,"token"))&&osg_copy(settings.token,sizeof(settings.token),osg_string(reply,"token"));
            if(ok){mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));
                if(settings.persist&&!osg_store_save(&settings)){cJSON_Delete(reply);fail("protected-save-failed-repair-required");return;}
                state=AUTH;report("paired");}
        }else if(ok){ok=equal(osg_string(reply,"type"),"pair-pending")&&equal(osg_string(reply,"requestId"),pending);next_request=millis()+400;}
    }else if(state==AUTH){
        if(!open_socket()){fail("host-tls-or-connection-failed");return;}
        v=message("auth");cJSON_AddStringToObject(v,"token",settings.token);
        ok=request(v,&reply,1000)&&identity(reply,"authenticated")&&choose(reply);
    }else if(state==WAIT_ASSIGNMENT){
        /* Refresh through a new authenticated connection, never authenticate twice. */
        osg_socket_close(&connection);state=AUTH;next_request=millis();return;
    }else if(state==OPEN){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        bool busy=*ui.drawing_panel==&images[1-image_index].panel;
        xSemaphoreGive(ui.mutex);if(busy)return;
        v=message("open");cJSON_AddStringToObject(v,"instanceId",assigned);cJSON_AddStringToObject(v,"publicationHash",publication);cJSON_AddBoolToObject(v,"takeover",false);
        ok=request(v,&reply,3000)&&osg_open_identity(reply,settings.host,settings.companion,assigned,publication,&session);
        if(ok){
            osg_image *candidate=&images[1-image_index];
            const cJSON *p=cJSON_GetObjectItemCaseSensitive(reply,"publication");
            ok=osg_decode_publication(p,publication,osg_platform_crypto,candidate,ui.runtime->display.width,ui.runtime->display.height);
            if(!ok){cJSON_Delete(reply);fail("panel-unsupported-corrupt-or-over-budget");return;}
            if(ok){xSemaphoreTake(ui.mutex,portMAX_DELAY);
                ok=osg_init(ui.runtime,&candidate->panel,ui.runtime->display,NULL,NULL);
                if(ok){image_index=1-image_index;osg_connected(ui.runtime,true);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());remote_armed=false;input_sequence=0;*ui.dirty=true;}
                xSemaphoreGive(ui.mutex);}
            if(ok){state=LIVE;next_request=millis();report("live-disarmed");}
        }
    }else if(state==LIVE){
        if(millis()>=next_request){
            if(session.heartbeat>=UINT64_MAX-1){fail("session-counter-exhausted");return;}
            char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++session.heartbeat);
            const uint64_t started=millis();
            ok=request(session_message("heartbeat",sequence),&reply,700)&&acknowledgement(reply,sequence,started);
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
                cJSON_AddBoolToObject(change,"value",edges[i].down);cJSON_AddItemToArray(changes,change);
            }
            const uint64_t started=millis();
            ok=request(v,&reply,400)&&acknowledgement(reply,sequence,started);
            xSemaphoreTake(ui.mutex,portMAX_DELAY);
            if(ok)ok=osg_input_accept(&input,epoch,count);
            xSemaphoreGive(ui.mutex);next_input_request=millis()+OSG_INPUT_BATCH_INTERVAL_MS;
        }
    }
    cJSON_Delete(reply);if(!ok)fail("host-rejected-or-invalid-reply-stop-and-reconnect");
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
    if(equal(type,"status")){public_status(osg_string(v,"requestId"));return;}
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
    if(equal(type,"forget")){stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));report(osg_store_forget()?"forgotten":"protected-store-delete-failed");return;}
    if(equal(type,"connect")){
        const char *instance=osg_string(v,"instanceId");
        if(!osg_secret(settings.token)||(instance&&*instance&&!osg_stable_id(instance))){report("pair-first-or-invalid-instance");return;}
        stop();if(instance)osg_copy(settings.instance,sizeof(settings.instance),instance);state=AUTH;next_request=millis();return;
    }
    if(equal(type,"provision")){
        osg_settings candidate;
        if(!osg_settings_parse(v,&candidate)){mbedtls_platform_zeroize(&candidate,sizeof(candidate));report("invalid-provisioning-record");return;}
        if(candidate.persist&&!osg_store_available()){mbedtls_platform_zeroize(&candidate,sizeof(candidate));report("protected-storage-unavailable-persistence-rejected");return;}
        stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));settings=candidate;mbedtls_platform_zeroize(&candidate,sizeof(candidate));
        if(!clock_from(v)){fail("trusted-clock-invalid");return;}
        install_identity();pair_deadline=millis()+60000;
    }else if(equal(type,"resume")){
        stop();osg_wifi_stop();mbedtls_platform_zeroize(&settings,sizeof(settings));
        if(!clock_from(v)||!osg_store_load(&settings)){fail("trusted-clock-or-protected-installation-unavailable");return;}
    }else{report("unsupported-command");return;}
    if(!osg_wifi_start(&settings)){osg_wifi_stop();fail("wifi-connection-failed");return;}
    state=settings.token[0]?AUTH:PAIR_BEGIN;next_request=millis();report(phases[state]);
}
static size_t serial_used;
static bool serial_overflow;
static void serial_byte(int c){
    if(c=='\n'||c=='\r'){
        if(serial_overflow){report("provisioning-line-too-large");serial_overflow=false;}
        else if(serial_used){serial_line[serial_used]=0;cJSON *v=osg_json_parse(serial_line,serial_used);if(v){command(v);cJSON_Delete(v);}else report("invalid-command-json");}
        mbedtls_platform_zeroize(serial_line,sizeof(serial_line));serial_used=0;
    }else if(!serial_overflow){if(serial_used<OSG_PROVISION_BYTES)serial_line[serial_used++]=(char)c;else serial_overflow=true;}
}
static void network_task(void *arg){
    (void)arg;osg_json_init();(void)fcntl(fileno(stdin),F_SETFL,O_NONBLOCK);report("serial-provisioning-ready");
    for(;;){int c;while((c=getchar())!=EOF)serial_byte(c);clearerr(stdin);advance();vTaskDelay(pdMS_TO_TICKS(10));}
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
    if(!success)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
}
void osg_network_start(osg_network_ui value){ui=value;osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());if(xTaskCreate(network_task,"companion-network",24576,NULL,4,NULL)!=pdPASS)report("network-task-allocation-failed");}
