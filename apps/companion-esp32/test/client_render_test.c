#define _DEFAULT_SOURCE
#include <sys/time.h>
#include <time.h>
static int fake_settimeofday(const struct timeval *,const struct timezone *);
#define settimeofday fake_settimeofday
#include "../network/client.c"
#undef settimeofday
#include "render_host_fixture.h"

static void paint(bool success);
static uint64_t fake_ms;static unsigned mutex_depth,release_after;
static bool network_ok=true,store_ok=true,assignment_available=true,obsolete,malformed_open,malformed_feedback;
static unsigned opens,closes,baselines,source_inputs,auths,feedback_sequence,wire_requests;
static bool host_armed,host_button;static host management;static cJSON *legacy_publication;
static bool legacy_mode,maximum_health;
static osg_runtime runtime;static bool dirty;static const osg_panel *drawing;
static char host_session[65];
static int fake_settimeofday(const struct timeval *v,const struct timezone *z){(void)z;return v->tv_sec>0?0:-1;}
int64_t esp_timer_get_time(void){return (int64_t)fake_ms*1000;}
void vTaskDelay(unsigned ms){assert(!mutex_depth);fake_ms+=ms;if(release_after&&!--release_after){engine_drawing=false;drawing=NULL;}if(using_scene&&!engine_drawing&&!engine_suspended&&render_engine.active&&!scene_disarmed_presented&&!render_engine.scene.failed)paint(true);}
int xTaskCreate(void (*fn)(void *),const char *name,unsigned bytes,void *arg,unsigned priority,void *handle){(void)fn;(void)name;(void)bytes;(void)arg;(void)priority;(void)handle;return pdPASS;}
int xSemaphoreTake(SemaphoreHandle_t s,unsigned wait){(void)s;(void)wait;assert(!mutex_depth);mutex_depth++;return 1;}
int xSemaphoreGive(SemaphoreHandle_t s){(void)s;assert(mutex_depth==1);mutex_depth--;return 1;}
void esp_fill_random(void *p,size_t n){memset(p,42,n);}
void mbedtls_platform_zeroize(void *p,size_t n){memset(p,0,n);}
cJSON *osg_serial_take_command(void){return NULL;}
cJSON *osg_wifi_scan(void){return cJSON_CreateArray();}
bool osg_serial_start(void){return true;}
void osg_serial_health_poll(void){}
void osg_serial_health_snapshot(osg_serial_health *out){if(out)memset(out,maximum_health?255:0,sizeof(*out));}
bool osg_trusted_ca(const osg_settings *s,unsigned char *p,size_t n,size_t *out){(void)s;(void)p;(void)n;(void)out;return true;}
bool osg_socket_open(osg_socket *s,const osg_settings *value){(void)value;assert(!mutex_depth);if(network_ok)s->ws=s;return network_ok;}
void osg_socket_close(osg_socket *s){assert(!mutex_depth);host_armed=host_button=false;memset(s,0,sizeof(*s));}
bool osg_wifi_clock(void){return true;}
bool osg_wifi_start(const osg_settings *s){(void)s;return network_ok;}
void osg_wifi_stop(void){}
bool osg_store_available(void){return true;}
bool osg_store_save(const osg_settings *s){(void)s;return store_ok;}
bool osg_store_load(osg_settings *s){(void)s;return false;}
bool osg_store_forget(void){return store_ok;}
static const char *source_hash(const fixture *f,char out[72]){cJSON *m=osg_json_parse(f->manifest,f->manifest_bytes);assert(m&&osg_copy(out,72,osg_string(m,"sourcePublicationHash")));cJSON_Delete(m);return out;}
static void compact_feedback(cJSON *reply){
    cJSON *v=cJSON_AddObjectToObject(reply,"renderFeedback");cJSON_AddNumberToObject(v,"v",malformed_feedback?1:2);cJSON_AddStringToObject(v,"g","1");char sequence[21];snprintf(sequence,sizeof(sequence),"%u",++feedback_sequence);cJSON_AddStringToObject(v,"s",sequence);
    bool snapshot=!session.feedback_seen;cJSON_AddNumberToObject(v,"m",snapshot?0:1);cJSON *bindings=snapshot?cJSON_AddArrayToObject(v,"b"):NULL,*rows=cJSON_AddArrayToObject(v,"x");
    unsigned output=0;for(size_t i=0;i<runtime.panel->widget_count;i++){const osg_widget *w=&runtime.panel->widgets[i];if(w->kind==OSG_BUTTON)continue;bool duplicate=false;for(size_t j=0;j<i;j++)if(runtime.panel->widgets[j].kind!=OSG_BUTTON&&!strcmp(runtime.panel->widgets[j].reference,w->reference))duplicate=true;
        if(!duplicate){if(bindings)cJSON_AddItemToArray(bindings,cJSON_CreateNumber(output));cJSON *row=cJSON_CreateArray();cJSON_AddItemToArray(rows,row);cJSON_AddItemToArray(row,cJSON_CreateNumber(output));cJSON_AddItemToArray(row,cJSON_CreateNumber(0));cJSON_AddItemToArray(row,cJSON_CreateNumber(2));cJSON_AddItemToArray(row,cJSON_CreateNumber(0));cJSON_AddItemToArray(row,cJSON_CreateNumber(0));cJSON_AddItemToArray(row,w->kind==OSG_INDICATOR?cJSON_CreateBool(true):cJSON_CreateNumber(42));}output++;
    }
}
bool osg_socket_request(osg_socket *s,cJSON *value,cJSON **out,unsigned timeout){
    (void)s;assert(!mutex_depth);wire_requests++;const char *type=osg_string(value,"type");assert(type);if(!strncmp(type,"render-",7)&&strcmp(type,"render-open"))return fake_request(&management,value,out,timeout);
    *out=reply_base("ack");cJSON *r=*out;char source[72];source_hash(management.package,source);if(legacy_mode)assert(osg_copy(source,72,osg_string(legacy_publication,"publicationHash")));
    if(!strcmp(type,"auth")){auths++;replace_text(r,"type","authenticated");cJSON_AddStringToObject(r,"hostId","host");cJSON_AddStringToObject(r,"companionId","screen");cJSON *list=cJSON_AddArrayToObject(r,"assignments");if(assignment_available){cJSON *a=cJSON_CreateObject();cJSON_AddItemToArray(list,a);cJSON_AddStringToObject(a,"instanceId","panel");cJSON_AddStringToObject(a,"publicationHash",source);}}
    else if(!strcmp(type,"render-open")||!strcmp(type,"open")){
        if(!strcmp(type,"render-open")){assert(render_engine.scene.ready&&!engine_suspended);assert(!strcmp(osg_string(value,"packageHash"),management.package->hash));}opens++;feedback_sequence=0;host_armed=host_button=false;
        replace_text(r,"type","session-open");cJSON_AddStringToObject(r,"hostId","host");cJSON_AddStringToObject(r,"companionId","screen");cJSON_AddStringToObject(r,"controllerInstanceId","panel");cJSON_AddStringToObject(r,"clientId","client");snprintf(host_session,sizeof(host_session),"session-%u",opens);cJSON_AddStringToObject(r,"sessionId",host_session);cJSON_AddStringToObject(r,"generation","1");cJSON_AddStringToObject(r,"publicationHash",source);
        if(!strcmp(type,"render-open")){cJSON_AddStringToObject(r,"packageHash",management.package->hash);cJSON_AddStringToObject(r,"profileHash",malformed_open?management.package->hash:osg_engine_profile_hash(&render_engine));cJSON *m=osg_json_parse(management.package->manifest,management.package->manifest_bytes);assert(m);cJSON_AddStringToObject(r,"semanticDescriptorHash",osg_string(m,"semanticDescriptorHash"));cJSON_Delete(m);}else{assert(legacy_publication);cJSON_AddItemToObject(r,"publication",cJSON_Duplicate(legacy_publication,true));}
    }else{
        assert(!strcmp(type,"heartbeat")||!strcmp(type,"baseline")||!strcmp(type,"input")||!strcmp(type,"close"));
        if(obsolete&&!strcmp(type,"heartbeat")){replace_text(r,"type","error");cJSON_AddStringToObject(r,"error","Session is obsolete; explicitly reconnect.");cJSON_Delete(value);return false;}
        if(!strcmp(type,"baseline")){assert(!host_armed);baselines++;host_armed=true;const cJSON *values=cJSON_GetObjectItemCaseSensitive(value,"values");assert(cJSON_IsObject(values));for(const cJSON *p=values->child;p;p=p->next)assert(cJSON_IsFalse(p));}
        if(!strcmp(type,"input")){assert(host_armed);source_inputs++;const cJSON *changes=cJSON_GetObjectItemCaseSensitive(value,"changes");assert(cJSON_IsArray(changes));for(const cJSON *p=changes->child;p;p=p->next)host_button=cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(p,"value"));}
        if(!strcmp(type,"close")){closes++;host_armed=host_button=false;replace_text(r,"type","closed");cJSON_AddNullToObject(r,"sequence");}else cJSON_AddStringToObject(r,"sequence",osg_string(value,"sequence"));
        cJSON_AddStringToObject(r,"clientId","client");cJSON_AddStringToObject(r,"sessionId",host_session);cJSON_AddStringToObject(r,"generation","1");cJSON_AddStringToObject(r,"ackStage","source-accepted");cJSON_AddBoolToObject(r,"submitted",false);
        if(render_session)compact_feedback(r);else{cJSON_AddNullToObject(r,"feedbackSequence");cJSON_AddNullToObject(r,"feedback");}
    }
    cJSON_Delete(value);return true;
}
static bool failed_read(void *context,uint32_t offset,void *out,size_t n){(void)context;(void)offset;(void)out;(void)n;return false;}
static bool draw_pixels(void *context,int x,int y,int width,int height,const uint16_t *p){assert(!mutex_depth&&engine_drawing);return pixels(context,x,y,width,height,p);}
static void paint(bool success){
    static osg_runtime snapshot;osg_engine *engine=NULL;assert(osg_network_scene_acquire(&snapshot,&engine)==1&&engine==&render_engine);(void)osg_engine_update_from_runtime(engine,&snapshot,fake_ms);
    unsigned slices=0;while(engine->scene.pending){assert(++slices<2000);unsigned before=pixel_writes;osg_engine_render_slice(engine,3,draw_pixels,engine);assert(pixel_writes-before<=3);fake_ms++;}
    if(engine->scene.awaiting_present){osg_engine_presented(engine,success);}osg_network_render(snapshot.panel,true,success);osg_network_scene_release(true);
}
static void connect_package(const fixture *f){stop();management=(host){.package=f,.engine=&render_engine};memset(&settings,0,sizeof(settings));osg_copy(settings.host,sizeof(settings.host),"host");osg_copy(settings.companion,sizeof(settings.companion),"screen");osg_copy(settings.token,sizeof(settings.token),"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA");state=AUTH;next_request=fake_ms;legacy_mode=false;assignment_available=true;obsolete=malformed_open=malformed_feedback=false;}
static void transfer_to_paint(void){advance();assert(state==OPEN);advance();assert(state==RENDER_TRANSFER&&engine_suspended);assert(osg_network_scene_acquire(NULL,NULL)==0);unsigned limit=0;while(state==RENDER_TRANSFER){fake_ms+=10;advance();assert(++limit<5000);}assert(state==RENDER_PAINT&&!engine_suspended&&!runtime.connected&&!runtime.armed);}
static void open_after_paint(void){unsigned before=opens;advance();assert(state==RENDER_PAINT&&opens==before);paint(true);assert(render_engine.scene.ready&&!scene_hits.ready);advance();assert(state==RENDER_OPEN&&opens==before);advance();assert(state==LIVE&&opens==before+1&&runtime.connected&&!runtime.armed&&render_session);paint(true);assert(scene_hits.ready);osg_network_touch(true,false,0,0);}
static void serial_text(const char *value){for(;*value;value++)serial_byte((unsigned char)*value);}
static void arm_live(void){assert(state==LIVE&&!runtime.armed);arm_when_ready();assert(runtime.armed&&remote_armed&&host_armed);advance();}
int main(int argc,char **argv){
    assert(argc==4);osg_json_init();size_t legacy_bytes;char *legacy_raw=load(argv[3],&legacy_bytes);legacy_publication=osg_json_parse(legacy_raw,legacy_bytes);free(legacy_raw);assert(legacy_publication);sha=EVP_MD_CTX_new();assert(sha);unsigned board=!strcmp(argv[2],"waveshare");const char *name=board?"waveshare":"sunton",*profile=board?"waveshare-lcd7-s3":"sunton-3248s035r";unsigned width=board?800:480,height=board?480:320;
    fixture a=fixture_load(argv[1],name,"A"),b=fixture_load(argv[1],name,"B");flash_size=board?0x600000:0x200000;memset(flash_bytes,255,sizeof(flash_bytes));assert(osg_engine_initialize_io(&render_engine,profile,(uint16_t)width,(uint16_t)height,io())==OSG_CONTENT_NOT_FOUND);
    char initial_source[72];source_hash(&a,initial_source);osg_page page={"boot","Boot",(uint16_t)width,(uint16_t)(height-32)};osg_widget widget={.id="boot",.reference="boot",.output_id="",.label="Boot",.prefix="",.suffix="",.unit="",.kind=OSG_BUTTON,.bounds={.1,.1,.2,.2}};osg_panel panel={"boot",initial_source,&page,&widget,1,1};assert(osg_init(&runtime,&panel,(osg_display){NULL,(int)width,(int)height,fill,text},NULL,NULL));osg_network_start((osg_network_ui){&runtime,(void *)1,&dirty,&drawing,true});
    assert(osg_network_scene_acquire(NULL,NULL)==-1);connect_package(&a);transfer_to_paint();assert(management.reads>0);open_after_paint();advance();assert(session.feedback_seen&&runtime.samples[render_engine.output_widgets[0]].available);paint(true);arm_live();
    osg_engine_hit control=scene_hits.hits[0];assert(control.widget!=OSG_ENGINE_NONE);fake_ms++;osg_network_touch(true,true,control.x+1,control.y+1);advance();assert(source_inputs==1&&host_button);fake_ms+=20;osg_network_touch(true,false,0,0);advance();assert(source_inputs==2&&!host_button);
    // A split management command retains parser ownership before input/heartbeat/nav work.
    serial_text("{\"command\":\"sta");unsigned wire_before=wire_requests;navigation_pending=0;fake_ms+=30;work_after_serial();assert(state==LIVE&&runtime.armed&&wire_requests==wire_before&&navigation_pending==0);navigation_pending=-1;
    serial_text("tus\"}\n");work_after_serial();assert(state==LIVE&&!serial_used&&!serial_overflow&&runtime.armed);
    unsigned before=flash_writes,old_open=opens;connect_package(&a);transfer_to_paint();assert(!management.reads&&!delivery.received&&flash_writes==before);open_after_paint();assert(opens==old_open+1&&!runtime.armed);
    // A source assignment change invalidates the old input session before downloading B.
    obsolete=true;next_request=fake_ms;advance();assert(state==AUTH&&!runtime.connected&&!runtime.armed&&!render_session);obsolete=false;management=(host){.package=&b,.engine=&render_engine};fake_ms+=500;transfer_to_paint();open_after_paint();assert(!strcmp(render_engine.package_hash,b.hash)&&!runtime.armed);
    // Navigation releases input, closes the old session and obtains a fresh cached grant.
    arm_live();unsigned sent=source_inputs,closed=closes,opened=opens;scene_hits.hits[scene_hits.count++]=(osg_engine_hit){0,runtime.page,OSG_ENGINE_NONE,runtime.page,0,(int16_t)(height-60),44,44};
    osg_network_touch(true,true,1,(int)height-59);advance();assert(state==LIVE&&runtime.armed);osg_network_touch(true,false,1,(int)height-59);assert(navigation_pending==0&&!runtime.armed);navigate_scene();assert(state==AUTH&&closes==closed+1&&source_inputs==sent);management=(host){.package=&b,.engine=&render_engine};transfer_to_paint();open_after_paint();assert(opens==opened+1&&!runtime.armed&&!management.reads);
    // Every cache/geometry guard rejects a source edge when disarmed; navigation is local only.
    paint(true);unsigned edges=source_inputs;osg_engine_hit_cache saved_hits=scene_hits;
    osg_network_touch(true,true,-1,0);osg_network_touch(true,false,0,0);
    osg_network_touch(true,true,(int)width,(int)height);osg_network_touch(true,false,0,0);
    scene_hits.count=0;osg_network_touch(true,true,control.x+1,control.y+1);osg_network_touch(true,false,0,0);scene_hits=saved_hits;
    scene_hits.ready=false;osg_network_touch(true,true,control.x+1,control.y+1);osg_network_touch(true,false,0,0);scene_hits=saved_hits;
    scene_hits.page=1;osg_network_touch(true,true,control.x+1,control.y+1);osg_network_touch(true,false,0,0);scene_hits=saved_hits;
    scene_hits.hits[0].page=1;osg_network_touch(true,true,control.x+1,control.y+1);osg_network_touch(true,false,0,0);scene_hits=saved_hits;
    engine_suspended=true;osg_network_touch(true,true,control.x+1,control.y+1);osg_network_touch(true,false,0,0);assert(osg_network_scene_acquire(NULL,NULL)==0);engine_suspended=false;render_engine.active=false;assert(osg_network_scene_acquire(NULL,NULL)==0);render_engine.active=true;
    osg_network_touch(false,true,control.x+1,control.y+1);osg_network_touch(false,false,0,0);assert(source_inputs==edges);scene_hits=saved_hits;
    navigation_pending=-1;navigate_scene();navigation_pending=99;navigate_scene();engine_suspended=true;navigation_pending=0;navigate_scene();engine_suspended=false;
    public_status("render-status");osg_network_scene_release(false);
    // The same parser guard cancels a held render source within100ms without a new ACK.
    paint(true);osg_network_touch(true,false,0,0);arm_live();osg_engine_hit held=scene_hits.hits[0];fake_ms++;osg_network_touch(true,true,held.x+1,held.y+1);advance();assert(host_button);
    serial_text("{\"command\":\"sta");wire_before=wire_requests;uint64_t last_ack=input.last_ack_ms;fake_ms+=99;work_after_serial();assert(host_button&&runtime.armed&&wire_requests==wire_before&&input.last_ack_ms==last_ack);
    fake_ms++;work_after_serial();assert(state==FAULT&&!runtime.armed&&!host_button&&!host_armed&&wire_requests==wire_before);serial_text("{\"command\":\"connect\"}\n");assert(state==FAULT&&!serial_used&&!serial_overflow);
    connect_package(&b);transfer_to_paint();open_after_paint();
    // A framebuffer failure suppresses hits and cannot become an armed session.
    connect_package(&a);transfer_to_paint();paint(false);advance();assert(state==RENDER_PAINT&&!scene_hits.ready);fake_ms=next_request;advance();assert(state==FAULT&&!runtime.armed&&!host_armed);
    osg_content_release(&render_engine.active_view);assert(osg_engine_initialize_io(&render_engine,profile,(uint16_t)width,(uint16_t)height,io())==OSG_CONTENT_OK);scene_disarmed_presented=false;
    connect_package(&b);transfer_to_paint();fake_ms=next_request;old_open=opens;advance();assert(state==FAULT&&opens==old_open&&!host_armed);
    // Malformed package/open/feedback replies close the connection and retain safe cached content.
    connect_package(&a);management.stage=2;management.mutation=6;advance();advance();while(state==RENDER_TRANSFER){fake_ms+=10;advance();}assert(state==FAULT&&!host_armed);
    connect_package(&b);transfer_to_paint();malformed_open=true;paint(true);advance();advance();assert(state==FAULT&&!runtime.armed);malformed_open=false;
    connect_package(&b);transfer_to_paint();open_after_paint();malformed_feedback=true;next_request=fake_ms;advance();assert(state==FAULT&&!runtime.armed);malformed_feedback=false;
    // Main-frame suspension waits without holding the UI mutex, and times out closed.
    connect_package(&b);advance();engine_drawing=true;drawing=runtime.panel;release_after=3;advance();assert(state==RENDER_TRANSFER&&engine_suspended&&!engine_drawing);stop();
    connect_package(&b);advance();engine_drawing=true;drawing=runtime.panel;advance();assert(state==FAULT&&!runtime.armed);engine_drawing=false;drawing=NULL;
    connect_package(&b);assignment_available=false;advance();assert(state==WAIT_ASSIGNMENT);fake_ms=next_request;advance();assert(state==AUTH);assignment_available=true;advance();assert(state==OPEN);stop();
    // Explicit legacy negotiation remains usable without trusting render package data.
    connect_package(&b);legacy_mode=true;management.stage=1;management.mutation=1;advance();advance();assert(state==RENDER_TRANSFER);advance();assert(state==LIVE&&!using_scene&&!render_session&&!runtime.armed);osg_network_touch(true,false,0,0);advance();public_status("legacy-status");stop();
    // A package transfer aborted mid-erase keeps the last committed scene.
    connect_package(&a);advance();advance();advance();advance();assert(render_engine.staging);stop();assert(!render_engine.staging&&!runtime.armed);
    // Cached scene read failures synchronously invalidate both hit and presentation grants.
    connect_package(&b);transfer_to_paint();open_after_paint();render_engine.store.io.read=failed_read;osg_network_scene_release(true);assert(render_engine.failed&&render_engine.scene.failed&&!scene_hits.ready&&!scene_presented_ready);render_engine.store.io.read=read_flash;stop();
    // Legal escaped names/SSID and maximum counters still fit the bounded record.
    maximum_health=true;memset(settings.name,1,80);settings.name[80]=0;memset(settings.ssid,1,32);settings.ssid[32]=0;memset(settings.companion,'c',64);settings.companion[64]=0;memset(session.session,'s',64);session.session[64]=0;osg_copy(session.generation,sizeof(session.generation),"18446744073709551615");input.epoch=UINT64_MAX;
    char maximum_id[65];memset(maximum_id,'r',64);maximum_id[64]=0;state=LIVE;public_status(maximum_id);state=OFF;
    cJSON_Delete(legacy_publication);assert(!mutex_depth&&!osg_json_allocated());osg_content_release(&render_engine.active_view);fixture_free(&a);fixture_free(&b);EVP_MD_CTX_free(sha);puts("Real client/engine cold-warm delivery, presentation gate, compact feedback, input, navigation, assignment refresh and failures passed");return 0;
}
