#include "platform.h"
#include "firmware.h"
#include "input.h"
#include "serial.h"
#include "diagnostics.h"
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
#ifdef OSG_RENDER_ENGINE
#include "render_delivery.h"
#include "render_feedback.h"
#include "../render/content_store_esp.h"
#endif
#ifdef ESP_PLATFORM
#include "freertos/task.h"
#endif

typedef enum { OFF, PAIR_BEGIN, PAIR_POLL, AUTH, WAIT_ASSIGNMENT, OPEN, LIVE, FAULT,
    RENDER_TRANSFER, RENDER_PAINT, RENDER_OPEN } phase;
static osg_network_ui ui;
static osg_settings settings;
static osg_socket connection;
static osg_image images[2];
static unsigned image_index;
static osg_live_session session;
static osg_input input;
static bool remote_armed, installation_saved;
static uint64_t input_sequence, next_input_request;
static phase state;
static char pending[65], assigned[65], publication[72];
static uint64_t next_request, pair_deadline;
static uint64_t diagnostics_window_started;
static char serial_line[OSG_PROVISION_BYTES+1];
static const char *phases[]={"offline","pairing","awaiting-approval","authenticating","awaiting-assignment","opening-panel","live-disarmed","error",
    "downloading-render-package","presenting-render-package","opening-render-session"};
#ifdef OSG_RENDER_ENGINE
static osg_engine render_engine;
static osg_render_delivery delivery;
static osg_engine_hit_cache scene_hits;
static bool using_scene, engine_suspended, engine_drawing, render_session, scene_presented_ready, scene_disarmed_presented, scene_drawing_connected;
static int navigation_capture=-1,navigation_pending=-1;
static uint16_t requested_page;
static char requested_page_package[72];
_Static_assert(sizeof(osg_render_delivery)+sizeof(osg_engine_hit_cache)+sizeof(osg_content_esp)+256<=1536,"Render adapter state exceeds its reserved budget");
#endif
static uint64_t millis(void){return (uint64_t)(esp_timer_get_time()/1000);}
static void report(const char *status){printf("{\"status\":\"%s\"}\n",status);fflush(stdout);}
static void disconnected(void){xSemaphoreTake(ui.mutex,portMAX_DELAY);osg_input_cancel(&input,OSG_INPUT_CANCELLED);osg_connected(ui.runtime,false);*ui.dirty=true;xSemaphoreGive(ui.mutex);}
static void stop(void);
static void abort_connection(void){osg_socket_close(&connection);disconnected();
#ifdef OSG_RENDER_ENGINE
    if(render_engine.staging)(void)osg_engine_abort(&render_engine,true);
    xSemaphoreTake(ui.mutex,portMAX_DELAY);engine_suspended=false;render_session=false;scene_hits.ready=false;xSemaphoreGive(ui.mutex);
#endif
    state=OFF;remote_armed=false;input_sequence=0;mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));}
static void fail(const char *reason){abort_connection();state=FAULT;mbedtls_platform_zeroize(settings.secret,sizeof(settings.secret));report(reason);}
static cJSON *message(const char *type){cJSON *v=cJSON_CreateObject();if(v)cJSON_AddStringToObject(v,"type",type);return v;}
static bool request(cJSON *v,cJSON **reply,unsigned timeout){
    const int64_t started=esp_timer_get_time();
    const bool ok=osg_socket_request(&connection,v,reply,timeout);
    osg_diagnostics_record(OSG_DIAG_REQUEST,(uint64_t)(esp_timer_get_time()-started),0,ok);return ok;
}
/* Called with ui.mutex held, before monitoring or polling can reset evidence.
 * Source ACK age is deliberately based on request start, as are the leases. */
static uint64_t age_us(uint64_t now,uint64_t before){return now<before?UINT64_MAX:(now-before>UINT64_MAX/1000?UINT64_MAX:(now-before)*1000);}
static void input_health(void){
    if(input.mode!=OSG_INPUT_PREPARING&&input.mode!=OSG_INPUT_ARMED)return;
    const uint64_t now=millis();
    osg_diagnostics_health(input.count,input.count?age_us(now,input.queue[input.head].created_ms):0,
        age_us(now,input.last_poll_ms),age_us(now,input.last_ack_ms),input.rendering?age_us(now,input.render_start_ms):0);
}
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
    const bool close=state==LIVE&&(connection.ws||connection.direct);
    disconnected(); /* Stop local input before any potentially blocking I/O. */
    if(close){cJSON *reply=NULL;(void)request(session_message("close",NULL),&reply,300);cJSON_Delete(reply);}
    abort_connection();
}
static bool acknowledgement(cJSON *reply,const char *sequence,uint64_t started,bool input_ack){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    input_health();
    const uint64_t before=session.feedback_sequence,before_generation=session.binding_generation;const bool seen=session.feedback_seen;
    bool ok=osg_input_monitor(&input,millis());
#ifdef OSG_RENDER_ENGINE
    if(ok&&render_session)ok=osg_render_feedback_ack(reply,&session,sequence,ui.runtime,millis(),seen?512:3072);
    else
#endif
    if(ok)ok=osg_ack(reply,&session,sequence,ui.runtime,millis());
    if(ok){input.last_ack_ms=started;if(!seen||before!=session.feedback_sequence||before_generation!=session.binding_generation)*ui.dirty=true;}
    xSemaphoreGive(ui.mutex);
    const uint64_t elapsed=age_us(millis(),started);
    osg_diagnostics_record(OSG_DIAG_SOURCE_ACK,elapsed,0,ok);
    if(input_ack)osg_diagnostics_record(OSG_DIAG_INPUT_ACK,elapsed,0,ok);
    return ok;
}
static void public_diagnostics(const cJSON *command){
    /* Dumping several serial records can block the UART. Never put that work
     * inside an armed source's 250/600/1000 ms lease windows. */
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const bool active=remote_armed||input.mode==OSG_INPUT_PREPARING||input.mode==OSG_INPUT_ARMED;
    xSemaphoreGive(ui.mutex);
    if(active){report("diagnostics-requires-stopped-input");return;}
    if(cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(command,"reset"))){osg_diagnostics_reset();diagnostics_window_started=millis();report("diagnostics-reset");return;}
    osg_diagnostics d;osg_diagnostics_snapshot(&d);
    const char *id=osg_string(command,"requestId");if(!osg_stable_id(id))id="";
    printf("{\"status\":\"diagnostics-summary\",\"version\":1,\"requestId\":\"%s\",\"boardProfile\":\"%s\",\"firmwareVersion\":\"%s\","
        "\"sampledAtUptimeMs\":%" PRIu64 ",\"windowStartedUptimeMs\":%" PRIu64 ","
        "\"serialRequestTxBytes\":%" PRIu64 ",\"serialRequestRxBytes\":%" PRIu64 ",\"wsTxPayloadBytes\":%" PRIu64 ",\"wsRxPayloadBytes\":%" PRIu64 ","
        "\"memorySamples\":%" PRIu32 ",\"freeInternalBytes\":%" PRIu32 ",\"minimumInternalBytes\":%" PRIu32 ",\"largestInternalBytes\":%" PRIu32 ",\"smallestLargestInternalBytes\":%" PRIu32 ","
        "\"stackMinBytes\":[%" PRIu32 ",%" PRIu32 ",%" PRIu32 "],\"stackSamples\":[%" PRIu32 ",%" PRIu32 ",%" PRIu32 "],"
        "\"queueMax\":%" PRIu32 ",\"pendingInputMaxUs\":%" PRIu64 ",\"touchAgeMaxUs\":%" PRIu64 ",\"sourceAckAgeMaxUs\":%" PRIu64 ",\"renderAgeMaxUs\":%" PRIu64 "}\n",
        id,OSG_BOARD_PROFILE,OSG_FIRMWARE_VERSION,millis(),diagnostics_window_started,d.bytes[OSG_DIAG_SERIAL_TX],d.bytes[OSG_DIAG_SERIAL_RX],d.bytes[OSG_DIAG_WS_TX],d.bytes[OSG_DIAG_WS_RX],
        d.memory_samples,d.free_internal_bytes,d.minimum_internal_bytes,d.largest_internal_bytes,d.smallest_largest_internal_bytes,
        d.stack_min_bytes[0],d.stack_min_bytes[1],d.stack_min_bytes[2],d.stack_samples[0],d.stack_samples[1],d.stack_samples[2],
        d.queue_max,d.pending_input_max_us,d.touch_age_max_us,d.ack_age_max_us,d.render_age_max_us);
    for(unsigned i=0;i<OSG_DIAG_METRIC_COUNT;i++){
        const osg_diag_stat *s=&d.metrics[i];
        printf("{\"status\":\"diagnostics-metric\",\"version\":1,\"requestId\":\"%s\",\"metric\":\"%s\",\"count\":%" PRIu32 ",\"failed\":%" PRIu32 ",\"dropped\":%" PRIu32
            ",\"bytes\":%" PRIu64 ",\"totalUs\":%" PRIu64 ",\"maxUs\":%" PRIu64 ",\"p50UpperUs\":%" PRIu64 ",\"p95UpperUs\":%" PRIu64 ",\"p99UpperUs\":%" PRIu64 "}\n",
            id,osg_diagnostics_metric_name((osg_diag_metric)i),s->count,s->failed,s->dropped,s->bytes,s->total_us,s->max_us,
            osg_diagnostics_percentile(s,50),osg_diagnostics_percentile(s,95),osg_diagnostics_percentile(s,99));
    }
    printf("{\"status\":\"diagnostics-complete\",\"version\":1,\"requestId\":\"%s\"}\n",id);fflush(stdout);
}
static void public_status(const char *request_id){
    cJSON *v=cJSON_CreateObject();char encoded[1536],epoch[21];
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    const char *status=state==LIVE&&input.mode==OSG_INPUT_ARMED?"live-armed":phases[state];
    cJSON_AddStringToObject(v,"status",status);
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
#ifdef OSG_RENDER_ENGINE
    cJSON_AddBoolToObject(v,"renderStorageAvailable",render_engine.available);
    cJSON_AddStringToObject(v,"renderPackageHash",osg_engine_current_package(&render_engine));
    cJSON_AddNumberToObject(v,"renderBytesReceived",delivery.received);
    cJSON_AddNumberToObject(v,"renderBytesTotal",delivery.total);
#endif
    cJSON_AddStringToObject(v,"ssid",settings.ssid);
    cJSON_AddBoolToObject(v,"wifiPasswordSaved",settings.password[0]!=0);
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
    bool ok=request(v,&reply,700)&&acknowledgement(reply,"0",started,false);
    cJSON_Delete(reply);
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(ok)ok=osg_input_commit(&input,epoch,started,millis());
    *ui.dirty=true;xSemaphoreGive(ui.mutex);
    if(!ok){fail("baseline-not-confirmed-or-touch-changed-reconnect-required");return;}
    input_sequence=0;next_input_request=next_request=millis();report("live-armed");
}
/* Connection alone is insufficient until the rendered panel and released touch
 * are healthy. Reuse the same session/epoch-bound neutral-baseline handshake. */
static void arm_when_ready(void){
    if(state!=LIVE||remote_armed)return;
    char epoch[21];cJSON *ticket=NULL;
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(osg_input_can_arm(&input,millis())){
        ticket=cJSON_CreateObject();
        snprintf(epoch,sizeof(epoch),"%" PRIu64,input.epoch);
        cJSON_AddStringToObject(ticket,"sessionId",session.session);
        cJSON_AddStringToObject(ticket,"generation",session.generation);
        cJSON_AddStringToObject(ticket,"publicationHash",session.publication);
        cJSON_AddStringToObject(ticket,"armEpoch",epoch);
    }
    xSemaphoreGive(ui.mutex);
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
    if(!selected){state=WAIT_ASSIGNMENT;next_request=millis()+2000;report("choose-or-assign-one-instance-in-cockpit");return true;}
    return osg_copy(assigned,sizeof(assigned),osg_string(selected,"instanceId"))&&osg_copy(publication,sizeof(publication),osg_string(selected,"publicationHash"))&&(state=OPEN,true);
}
static bool open_socket(void){return connection.ws||connection.direct||osg_socket_open(&connection,&settings);}
#ifdef OSG_RENDER_ENGINE
static bool suspend_scene(void){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);scene_hits.ready=false;scene_presented_ready=false;
    if(!using_scene)engine_suspended=true;
    xSemaphoreGive(ui.mutex);
    const uint64_t started=millis();
    for(;;){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        const bool busy=engine_drawing||*ui.drawing_panel!=NULL;
        const bool chrome_ready=!using_scene||scene_disarmed_presented;
        if(!busy&&chrome_ready){engine_suspended=true;xSemaphoreGive(ui.mutex);return true;}
        xSemaphoreGive(ui.mutex);
        if(millis()-started>=1500)return false;
        vTaskDelay(pdMS_TO_TICKS(1));
    }
}
static bool scene_panel(void){
    osg_image *candidate=&images[1-image_index];
    if(!osg_engine_make_panel(&render_engine,candidate))return false;
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    bool ok=osg_init(ui.runtime,&candidate->panel,ui.runtime->display,NULL,NULL);
    if(ok){image_index=1-image_index;osg_connected(ui.runtime,false);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());
        if(equal(requested_page_package,osg_engine_current_package(&render_engine))&&requested_page<candidate->panel.page_count)(void)osg_set_page(ui.runtime,requested_page);
        using_scene=true;scene_hits.ready=false;scene_presented_ready=false;scene_disarmed_presented=false;remote_armed=false;input_sequence=0;navigation_capture=-1;*ui.dirty=true;}
    engine_suspended=false;xSemaphoreGive(ui.mutex);return ok;
}
static bool delivery_request(void *context,cJSON *value,cJSON **reply,unsigned timeout){(void)context;return request(value,reply,timeout);}
static void navigate_scene(void){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);int page=navigation_pending;navigation_pending=-1;
    bool allowed=page>=0&&using_scene&&!engine_suspended&&(unsigned)page<ui.runtime->panel->page_count;
    xSemaphoreGive(ui.mutex);if(!allowed)return;
    const bool reconnect=state==LIVE;stop();
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    requested_page=(uint16_t)page;osg_copy(requested_page_package,sizeof(requested_page_package),osg_engine_current_package(&render_engine));
    (void)osg_set_page(ui.runtime,(size_t)page);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());scene_hits.ready=false;*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
    if(reconnect){state=AUTH;next_request=millis();}report("scene-page-changed-input-stopped");
}
#endif
static void advance(void){
    if(state==OFF||state==FAULT)return;
    bool send_input=false;
    if(state==LIVE){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);
        input_health();
        bool healthy=osg_input_monitor(&input,millis());
        send_input=remote_armed&&input.mode==OSG_INPUT_ARMED&&input.count&&millis()>=next_input_request;
        xSemaphoreGive(ui.mutex);
        if(!healthy){fail("input-cancelled-or-lease-expired-reconnect-required");return;}
    }
    if(millis()<next_request&&!send_input&&state!=RENDER_TRANSFER&&state!=RENDER_PAINT&&state!=RENDER_OPEN)return;
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
    }else if(state==AUTH){
        if(!open_socket()){fail("host-tls-or-connection-failed");return;}
        v=message("auth");cJSON_AddStringToObject(v,"token",settings.token);
        ok=request(v,&reply,3000)&&identity(reply,"authenticated")&&choose(reply);
    }else if(state==WAIT_ASSIGNMENT){
        /* Refresh through a new authenticated connection, never authenticate twice. */
        osg_socket_close(&connection);state=AUTH;next_request=millis();return;
    }else if(state==OPEN){
#ifdef OSG_RENDER_ENGINE
        if(render_engine.available){
            if(!suspend_scene()||!osg_delivery_start(&delivery,&render_engine,assigned,publication)){fail("render-engine-could-not-suspend");return;}
            state=RENDER_TRANSFER;return;
        }
#endif
#ifdef OSG_RENDER_ENGINE
legacy_open:
#endif
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
                if(ok){image_index=1-image_index;osg_connected(ui.runtime,true);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());remote_armed=false;input_sequence=0;*ui.dirty=true;
#ifdef OSG_RENDER_ENGINE
                    using_scene=false;engine_suspended=false;render_session=false;
#endif
                }
                xSemaphoreGive(ui.mutex);}
            if(ok){state=LIVE;next_request=millis();report("live-disarmed");}
        }
#ifdef OSG_RENDER_ENGINE
    }else if(state==RENDER_TRANSFER){
        osg_delivery_phase result=osg_delivery_step(&delivery,&render_engine,delivery_request,NULL,millis(),true);
        if(result==OSG_DELIVERY_ERROR){fail(delivery.error?delivery.error:"render-package-transfer-failed");return;}
        if(result==OSG_DELIVERY_LEGACY){state=OPEN;goto legacy_open;}
        if(result==OSG_DELIVERY_COMPLETE){
            if(!scene_panel()){fail("render-package-logical-panel-rejected");return;}
            state=RENDER_PAINT;next_request=millis()+3000;report("render-package-verified");
        }
        return;
    }else if(state==RENDER_PAINT){
        xSemaphoreTake(ui.mutex,portMAX_DELAY);bool ready=scene_presented_ready;xSemaphoreGive(ui.mutex);
        if(!ready){if(millis()>=next_request)fail("render-package-first-presentation-failed");return;}
        state=RENDER_OPEN;return;
    }else if(state==RENDER_OPEN){
        v=osg_delivery_open_request(&delivery,&render_engine);
        ok=v&&request(v,&reply,3000)&&osg_open_identity(reply,settings.host,settings.companion,assigned,publication,&session)&&
            equal(osg_string(reply,"packageHash"),delivery.package)&&equal(osg_string(reply,"profileHash"),osg_engine_profile_hash(&render_engine))&&
            equal(osg_string(reply,"semanticDescriptorHash"),delivery.semantic);
        if(ok){
            xSemaphoreTake(ui.mutex,portMAX_DELAY);osg_connected(ui.runtime,true);osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());
            render_session=true;scene_hits.ready=false;scene_disarmed_presented=false;remote_armed=false;input_sequence=0;*ui.dirty=true;xSemaphoreGive(ui.mutex);
            state=LIVE;next_request=millis();report("render-session-disarmed");
        }
#endif
    }else if(state==LIVE){
        if(millis()>=next_request&&!send_input){
            if(session.heartbeat>=UINT64_MAX-1){fail("session-counter-exhausted");return;}
            char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++session.heartbeat);
            const uint64_t started=millis();
            ok=request(session_message("heartbeat",sequence),&reply,700)&&acknowledgement(reply,sequence,started,false);
            next_request=millis()+250;
        }else if(send_input){
            osg_input_edge edges[OSG_INPUT_BATCH_EDGES];size_t count=0;uint64_t epoch=0;
            xSemaphoreTake(ui.mutex,portMAX_DELAY);
            ok=osg_input_peek(&input,edges,
#ifdef OSG_RENDER_ENGINE
                render_session?2:
#endif
                OSG_INPUT_BATCH_EDGES,&count,&epoch,millis());
            xSemaphoreGive(ui.mutex);
            if(!ok||!count||input_sequence>=UINT64_MAX-1){fail("input-backlog-or-counter-invalid-reconnect-required");return;}
            char sequence[21];snprintf(sequence,sizeof(sequence),"%" PRIu64,++input_sequence);
            v=session_message("input",sequence);cJSON *changes=cJSON_AddArrayToObject(v,"changes");
            for(size_t i=0;i<count;i++){
                cJSON *change=cJSON_CreateObject();cJSON_AddStringToObject(change,"controlId",ui.runtime->panel->widgets[edges[i].widget].reference);
                cJSON_AddBoolToObject(change,"value",edges[i].down);cJSON_AddItemToArray(changes,change);
            }
            const uint64_t started=millis();
            ok=request(v,&reply,400)&&acknowledgement(reply,sequence,started,true);
            xSemaphoreTake(ui.mutex,portMAX_DELAY);
            if(ok)ok=osg_input_accept(&input,epoch,count);
            xSemaphoreGive(ui.mutex);next_input_request=millis()+OSG_INPUT_BATCH_INTERVAL_MS;
        }
    }
    /* Cockpit invalidates the old session when it changes assignment.
     * Reauthenticate for the current assignment, cancelling all old input first. */
    bool assignment_refresh=!ok&&state==LIVE&&equal(osg_string(reply,"type"),"error")&&
        equal(osg_string(reply,"error"),"Session is obsolete; explicitly reconnect.");
    cJSON_Delete(reply);
    if(assignment_refresh){abort_connection();state=AUTH;next_request=millis()+500;report("refreshing-cockpit-assignment-disarmed");}
    else if(!ok)fail("host-rejected-or-invalid-reply-stop-and-reconnect");
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
    if(equal(type,"diagnostics")){public_diagnostics(v);return;}
    if(equal(type,"status")){public_status(osg_string(v,"requestId"));return;}
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
static size_t serial_used;
static bool serial_overflow;
static void serial_byte(int c){
    if(c=='\n'||c=='\r'){
        if(serial_overflow){report("provisioning-line-too-large");serial_overflow=false;}
        else if(serial_used){serial_line[serial_used]=0;cJSON *v=osg_json_parse(serial_line,serial_used);if(v){command(v);cJSON_Delete(v);}else report("invalid-command-json");}
        mbedtls_platform_zeroize(serial_line,sizeof(serial_line));serial_used=0;
    }else if(!serial_overflow){if(serial_used<OSG_PROVISION_BYTES)serial_line[serial_used++]=(char)c;else serial_overflow=true;}
}
static void update_screen(void){
    osg_screen_state screen=!settings.token[0] ? OSG_SCREEN_UNPAIRED :
        state==LIVE ? OSG_SCREEN_PANEL : state==WAIT_ASSIGNMENT ? OSG_SCREEN_NO_PANEL : OSG_SCREEN_DISCONNECTED;
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(ui.runtime->screen!=screen){ui.runtime->screen=screen;*ui.dirty=true;}
    xSemaphoreGive(ui.mutex);
}
static void network_task(void *arg){
    (void)arg;osg_json_init();
#ifdef OSG_RENDER_ENGINE
    if(suspend_scene()){
        (void)osg_engine_initialize(&render_engine,OSG_BOARD_PROFILE,ui.runtime->display.width,ui.runtime->display.height);
        if(render_engine.active){if(!scene_panel())report("cached-render-panel-rejected");else report("cached-render-package-restored-offline");}
        xSemaphoreTake(ui.mutex,portMAX_DELAY);engine_suspended=false;xSemaphoreGive(ui.mutex);
    }else report("render-storage-initialization-suspend-failed");
#endif
    if(osg_store_load(&settings)){
        installation_saved=true;
        if(settings.direct){state=AUTH;next_request=millis();}
        else if(!osg_wifi_start(&settings))report("saved-setup-wifi-unavailable");
        else if(!osg_wifi_clock())report("saved-setup-clock-unavailable");
        else {state=AUTH;next_request=millis();report("restoring-saved-setup");}
    }
    (void)fcntl(fileno(stdin),F_SETFL,O_NONBLOCK);report("serial-provisioning-ready");
    for(;;){cJSON *pending_command=osg_serial_take_command();if(pending_command){command(pending_command);cJSON_Delete(pending_command);}int c;while((c=getchar())!=EOF)serial_byte(c);clearerr(stdin);
#ifdef OSG_RENDER_ENGINE
        navigate_scene();
#endif
        advance();update_screen();arm_when_ready();
#ifdef ESP_PLATFORM
        static uint64_t last_stack;
        if(millis()-last_stack>=1000){osg_diagnostics_stack(OSG_DIAG_NETWORK_STACK,(uint32_t)uxTaskGetStackHighWaterMark(NULL));last_stack=millis();}
#endif
        vTaskDelay(pdMS_TO_TICKS(10));}
}
void osg_network_touch(bool valid,bool down,int x,int y){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    input_health();
    bool was_armed=ui.runtime->armed,was_arming=ui.runtime->arming;int previous=ui.runtime->active;
#ifdef OSG_RENDER_ENGINE
    if(using_scene&&down){
        bool hit=false;
        int destination=-1;
        if(!engine_suspended&&scene_hits.ready&&scene_hits.page==ui.runtime->page){
            for(unsigned i=0;i<scene_hits.count;i++){
                const osg_engine_hit *h=&scene_hits.hits[i];
                if(h->page==ui.runtime->page&&x>=h->x&&y>=h->y&&x<h->x+h->width&&y<h->y+h->height){
                    if(h->destination_page!=OSG_ENGINE_NONE)destination=h->destination_page;
                    else if(h->widget!=OSG_ENGINE_NONE)hit=true;
                    break;
                }
            }
        }
        if(!hit){x=-1;y=-1;}
        navigation_capture=valid?destination:-1;
    }else if(using_scene){
        if(valid&&navigation_capture>=0&&scene_hits.ready&&!engine_suspended){navigation_pending=navigation_capture;osg_input_cancel(&input,OSG_INPUT_CANCELLED);}
        navigation_capture=-1;
    }
#endif
    osg_input_poll(&input,valid,down,x,y,millis());
    if(was_armed!=ui.runtime->armed||was_arming!=ui.runtime->arming||previous!=ui.runtime->active)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
}
#ifdef OSG_RENDER_ENGINE
int osg_network_scene_acquire(osg_runtime *snapshot,osg_engine **engine){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    int result=-1;
    if(using_scene){
        result=0;
        if(!engine_suspended&&render_engine.active){
            *snapshot=*ui.runtime;*engine=&render_engine;engine_drawing=true;scene_drawing_connected=snapshot->connected;*ui.drawing_panel=snapshot->panel;*ui.dirty=false;result=1;
        }
    }else if(engine_suspended)result=0;
    xSemaphoreGive(ui.mutex);return result;
}
void osg_network_scene_release(bool publish_hits){
    osg_engine_hit_cache next={0};
    bool copied=publish_hits&&osg_engine_copy_hits(&render_engine,&next);
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    if(publish_hits){if(copied)scene_hits=next;else scene_hits.ready=false;scene_presented_ready=render_engine.scene.ready&&!render_engine.failed&&!render_engine.scene.failed;scene_disarmed_presented=scene_presented_ready&&!scene_drawing_connected;}
    engine_drawing=false;*ui.drawing_panel=NULL;xSemaphoreGive(ui.mutex);
}
#endif
void osg_network_render(const osg_panel *panel,bool finished,bool success){
    xSemaphoreTake(ui.mutex,portMAX_DELAY);
    input_health();
    osg_input_render(&input,panel,finished,success,millis());
    if(!success)*ui.dirty=true;
    xSemaphoreGive(ui.mutex);
}
void osg_network_start(osg_network_ui value){ui=value;ui.runtime->screen=OSG_SCREEN_UNPAIRED;osg_input_bind(&input,ui.runtime,ui.touch_capable,millis());if(!osg_serial_start()){report("serial-driver-initialization-failed");return;}if(xTaskCreate(network_task,"companion-network",24576,NULL,4,NULL)!=pdPASS)report("network-task-allocation-failed");}
