/* Native physical backend only. Configuration, acquisition, flash journals,
 * CDC parser, CAN transfer handling and descriptors are production sources. */
#include "opensimgear/gateway_config.h"
#include "opensimgear/gateway_layout.h"
#include "opensimgear/line.h"
#include "config_can.h"
#include "can_delivery.h"
#include "boot_session.h"
#include "example.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>

typedef struct { uint8_t pages[2][OSG_JOURNAL_PAGE_SIZE]; int fault; const char *name; uint32_t erases,programs; } flash_t;
typedef struct { osg_gateway_frame_t frame; bool gateway; } queued_t;
static struct {
    osg_gateway_t gateway;
    osg_cyphal_t gateway_can;
    osg_gateway_config_t config;
    osg_gateway_layout_t layout;
    osg_can_client_t module;
    osg_module_config_can_t module_config;
    osg_runtime_config_t runtime;
    osg_profile_state_t acquisition;
    osg_profile_sample_t sample;
    osg_can_delivery_t delivery;
    uint8_t report[OSG_REPORT_CAPACITY],gateway_descriptor[OSG_GW_DESCRIPTOR_CAPACITY];
    size_t descriptor_length;
    queued_t queue[4096]; unsigned queued;
    flash_t registry_flash,settings_flash,boot_flash;
    osg_gateway_identity_t gateway_identity,module_identity;
    osg_gateway_message_t old_input;
    osg_gateway_message_t captured_input;
    osg_gateway_delivery_t captured_delivery;
    uint8_t captured_packet[OSG_GW_MAX_REPORT_SIZE+1]; size_t captured_length;
    uint32_t now,module_reconnects,can_frames,acks,ignored_config,stale_rejected;
    bool disconnected,direct,drop_reply,old_input_valid,captured_input_valid;
    char response[OSG_GW_COCKPIT_RESPONSE_CAPACITY],active_config[4096];
} emulator;
static const char *state_directory;
static const char gateway_uid[]="00112233445566778899aabb";

static void disk_save(flash_t *flash) {
    if(!state_directory) return;
    char path[1024]; int n=snprintf(path,sizeof path,"%s/%s.bin",state_directory,flash->name);
    assert(n>0 && (size_t)n<sizeof path);
    FILE *file=fopen(path,"wb"); assert(file);
    assert(fwrite(flash->pages,sizeof flash->pages,1,file)==1); assert(!fclose(file));
}
static void flash_init(flash_t *flash,const char *name) {
    memset(flash->pages,255,sizeof flash->pages); flash->fault=-1; flash->name=name;
    if(!state_directory) return;
    char path[1024]; int n=snprintf(path,sizeof path,"%s/%s.bin",state_directory,name);
    assert(n>0 && (size_t)n<sizeof path); FILE *file=fopen(path,"rb");
    if(file) { assert(fread(flash->pages,sizeof flash->pages,1,file)==1); assert(!fclose(file)); }
}
static const uint8_t *flash_read(void *context,unsigned page) {
    flash_t *flash=context; return page<2 ? flash->pages[page] : NULL;
}
static bool interrupted(flash_t *flash) {
    if(flash->fault<0) return false;
    if(!flash->fault) { flash->fault=-1; return true; }
    flash->fault--; return false;
}
static bool flash_erase(void *context,unsigned page) {
    flash_t *flash=context; if(page>1) return false;
    flash->erases++;
    bool fail=interrupted(flash); memset(flash->pages[page],255,fail ? OSG_JOURNAL_PAGE_SIZE/2 : OSG_JOURNAL_PAGE_SIZE);
    disk_save(flash); return !fail;
}
static bool flash_program(void *context,unsigned page,unsigned offset,const uint8_t data[8]) {
    flash_t *flash=context; if(page>1 || offset%8 || offset+8>OSG_JOURNAL_PAGE_SIZE) return false;
    flash->programs++;
    bool fail=interrupted(flash); unsigned length=fail ? 4U : 8U;
    for(unsigned i=0;i<length;i++) {
        if((flash->pages[page][offset+i]&data[i])!=data[i]) return false;
        flash->pages[page][offset+i]&=data[i];
    }
    disk_save(flash); return !fail;
}
static osg_journal_io_t io(flash_t *flash) {
    return (osg_journal_io_t){.context=flash,.read=flash_read,.erase=flash_erase,.program=flash_program};
}
static bool enqueue(bool gateway,const osg_gateway_frame_t *frame) {
    if(emulator.queued>=sizeof emulator.queue/sizeof emulator.queue[0]) return false;
    emulator.queue[emulator.queued++]=(queued_t){.frame=*frame,.gateway=gateway}; return true;
}
static bool gateway_send(void *context,const osg_gateway_frame_t *frame) { (void)context; return enqueue(true,frame); }
static bool module_send(void *context,const osg_gateway_frame_t *frame) { (void)context; return enqueue(false,frame); }
static bool persist(void *context) {
    (void)context; uint8_t record[OSG_GW_REGISTRY_RECORD_SIZE];
    size_t n=osg_gateway_registry_encode(&emulator.gateway.pending,record,sizeof record);
    osg_journal_io_t flash=io(&emulator.registry_flash);
    return n && osg_journal_commit(&flash,record,n,emulator.gateway.epoch);
}
static void prepare_descriptor(void) {
    emulator.descriptor_length=osg_gateway_descriptor(&emulator.gateway,emulator.gateway_descriptor,sizeof emulator.gateway_descriptor);
    assert(emulator.descriptor_length);
}
static void rearm(void) {
    assert(osg_profile_init(&emulator.acquisition,&emulator.runtime.profile,&emulator.sample,emulator.now));
    osg_profile_neutral(&emulator.acquisition,emulator.report); osg_can_delivery_rearm(&emulator.delivery);
}
static void init_module_client(uint32_t boot) {
    osg_gateway_profile_t p; assert(osg_can_profile(&emulator.runtime.profile,&emulator.runtime.layout,&p));
    assert(osg_can_client_init(&emulator.module,&emulator.module_identity,&p,boot,module_send,NULL));
    emulator.module.configuration_generation=emulator.runtime.generation;
    assert(canard_set_node_id(&emulator.module.cyphal.canard,42));
    osg_module_config_can_init(&emulator.module_config,&emulator.module,&emulator.runtime);
    osg_can_client_usb(&emulator.module,emulator.direct); rearm();
}
static void boot_module(void) {
    osg_cyphal_destroy(&emulator.module.cyphal);
    uint32_t boot; osg_journal_io_t settings=io(&emulator.settings_flash),counter=io(&emulator.boot_flash);
    assert(osg_runtime_init(&emulator.runtime,&selected_profile,&settings));
    assert(osg_boot_session_begin(&counter,&boot)); init_module_client(boot);
}
static void boot_gateway(void) {
    osg_cyphal_destroy(&emulator.gateway_can);
    uint8_t record[OSG_GW_REGISTRY_RECORD_SIZE]; size_t n=0; uint32_t epoch=0;
    osg_gateway_registry_t registry,*restored=NULL; osg_journal_io_t flash=io(&emulator.registry_flash);
    if(osg_journal_load(&flash,record,sizeof record,&n,&epoch)) { assert(osg_gateway_registry_decode(record,n,&registry)); restored=&registry; }
    else assert(osg_journal_erased(&flash));
    assert(epoch<=UINT32_MAX-65536U); assert(osg_gateway_init(&emulator.gateway,epoch+65536U,restored));
    assert(persist(NULL)); assert(osg_cyphal_init(&emulator.gateway_can,&emulator.gateway_identity,emulator.gateway.epoch,
        "org.opensimgear.gateway",gateway_send,NULL));
    assert(canard_set_node_id(&emulator.gateway_can.canard,101));
    osg_gateway_config_cancel(&emulator.config); memset(&emulator.layout,0,sizeof emulator.layout);
    prepare_descriptor(); osg_gateway_set_running(&emulator.gateway,!emulator.disconnected);
}
static void pump(void) {
    for(unsigned i=0;i<emulator.queued;i++) {
        queued_t event=emulator.queue[i]; emulator.can_frames++;
        if(event.frame.deadline_valid && (int32_t)(emulator.now-event.frame.deadline_ms)>=0) continue;
        if(event.gateway) {
            uint32_t generation=emulator.module.generation;
            if(osg_can_client_receive(&emulator.module,&event.frame,emulator.now)) {
                emulator.acks++; osg_can_delivery_completed(&emulator.delivery,&emulator.acquisition,
                    emulator.module.ack_sequence,emulator.module.acknowledged,emulator.now);
            }
            if(generation!=emulator.module.generation) rearm();
        } else {
            osg_gateway_message_t m,reply;
            if(!osg_cyphal_receive(&emulator.gateway_can,&event.frame,emulator.now,&m)) continue;
            if(m.type==OSG_GW_CONFIG_RESPONSE) {
                if(emulator.drop_reply) emulator.ignored_config++;
                else (void)osg_gateway_config_receive(&emulator.config,&emulator.gateway,&m);
            } else {
                if(m.type==OSG_GW_INPUT) { emulator.old_input=m; emulator.old_input_valid=true; }
                if(osg_gateway_receive(&emulator.gateway,&m,emulator.now,&reply)==OSG_GW_RX_ASSIGNMENT)
                    assert(osg_cyphal_send(&emulator.gateway_can,&reply,emulator.now));
            }
        }
    }
    emulator.queued=0;
}
static void step(void) {
    emulator.now++; osg_runtime_tick(&emulator.runtime,emulator.now); osg_gateway_tick(&emulator.gateway,emulator.now);
    osg_gateway_layout_event_t event=osg_gateway_layout_tick(&emulator.layout,emulator.now,!emulator.config.busy);
    if(event==OSG_GATEWAY_LAYOUT_DETACH) {
        osg_gateway_set_running(&emulator.gateway,false); osg_gateway_apply_layout(&emulator.gateway); prepare_descriptor();
    } else if(event==OSG_GATEWAY_LAYOUT_ATTACH) osg_gateway_set_running(&emulator.gateway,!emulator.disconnected);
    emulator.sample.axes_fresh=emulator.runtime.layout.adc_mask;
    emulator.sample.expansion_available=emulator.runtime.active.expander_mask;
    osg_profile_update(&emulator.acquisition,&emulator.sample,emulator.now,
        osg_can_client_active(&emulator.module) || emulator.direct,emulator.report);
    osg_runtime_transform_report(&emulator.runtime,&emulator.acquisition,emulator.report);
    osg_can_delivery_observe(&emulator.delivery,&emulator.acquisition,emulator.module.next_sequence);
    uint8_t routed[OSG_GW_MAX_REPORT_SIZE]={0}; memcpy(routed,emulator.report,emulator.runtime.layout.length);
    osg_can_client_tick(&emulator.module,emulator.now,routed);
    osg_cyphal_tick(&emulator.gateway_can,emulator.now);
    osg_gateway_config_tick(&emulator.config,&emulator.gateway,&emulator.gateway_can,emulator.now);
    osg_module_config_can_tick(&emulator.module_config,emulator.now); pump();
    if(emulator.runtime.pending_apply && !osg_module_config_can_busy(&emulator.module_config)) {
        uint32_t boot=emulator.module.boot;
        assert(osg_runtime_finish_apply(&emulator.runtime)); emulator.module_reconnects++;
        (void)boot;
        assert(osg_can_profile(&emulator.runtime.profile,&emulator.runtime.layout,&emulator.module.profile));
        emulator.module.configuration_generation=emulator.runtime.generation;
        osg_can_client_reset(&emulator.module); rearm();
    }
    if(emulator.gateway.running) for(unsigned slot=0;slot<OSG_GW_MAX_MODULES;slot++) {
        uint8_t packet[OSG_GW_MAX_REPORT_SIZE+1]; packet[0]=(uint8_t)(slot+1);
        osg_gateway_delivery_t delivery; osg_gateway_message_t ack;
        size_t n=osg_gateway_delivery_prepare(&emulator.gateway,slot,packet+1,sizeof packet-1,&delivery);
        if(n && osg_gateway_delivery_complete(&emulator.gateway,&delivery,packet,n+1,&ack))
            assert(osg_cyphal_send(&emulator.gateway_can,&ack,emulator.now));
    }
    pump();
}
static void advance(unsigned milliseconds) { for(unsigned i=0;i<milliseconds;i++) step(); }
static void hex(const uint8_t *bytes,size_t n) { for(size_t i=0;i<n;i++) printf("%02x",bytes[i]); }
static bool probe_config(const char *field) {
    osg_can_client_t *c=&emulator.module;
    const char child[]="claim 98765";
    osg_gateway_message_t m={.type=OSG_GW_CONFIG_REQUEST,.identity=c->identity,.node=c->node,
        .boot_session=c->boot,.lease=c->lease,.gateway_epoch=c->epoch,.sequence=UINT32_C(900000),
        .config_total=sizeof child-1,.config_length=sizeof child-1};
    memcpy(m.config_data,child,sizeof child-1);
    uint8_t source=osg_cyphal_node_id(&emulator.gateway_can);
    if(!strcmp(field,"source")) assert(canard_set_node_id(&emulator.gateway_can.canard,99));
    else if(!strcmp(field,"boot")) m.boot_session++;
    else if(!strcmp(field,"lease")) m.lease++;
    else if(!strcmp(field,"epoch")) m.gateway_epoch++;
    else if(!strcmp(field,"node")) m.node=m.node==1?2:1;
    else if(!strcmp(field,"identity")) m.identity.bytes[0]^=1;
    else if(!strcmp(field,"reordered")) { m.config_total=48;m.config_offset=24;m.config_length=24; }
    else return false;
    assert(osg_cyphal_send(&emulator.gateway_can,&m,emulator.now));
    if(!strcmp(field,"source")) assert(canard_set_node_id(&emulator.gateway_can.canard,source));
    pump();advance(20);return true;
}
static void inspect(uint32_t id,bool ok,const char *error) {
    size_t n=osg_runtime_snapshot(&emulator.runtime,true,emulator.active_config,sizeof emulator.active_config); assert(n);
    printf("{\"id\":%u,\"ok\":%s,\"error\":",id,ok?"true":"false");
    if(error) printf("\"%s\"",error); else printf("null");
    printf(",\"emulator\":{\"clockMs\":%u,\"gatewayEpoch\":%u,\"moduleBoot\":%u,\"gatewayReconnects\":%u,\"moduleReconnects\":%u,\"canFrames\":%u,\"acks\":%u,\"ignoredConfigChunks\":%u,\"staleRejected\":%u,\"activeGeneration\":%u,\"pendingGeneration\":%u,\"settingsErases\":%u,\"settingsPrograms\":%u,\"descriptorHex\":\"",
        emulator.now,emulator.gateway.epoch,emulator.module.boot,emulator.layout.reconnects,emulator.module_reconnects,
        emulator.can_frames,emulator.acks,emulator.ignored_config,emulator.stale_rejected,
        emulator.gateway.active.generation,emulator.gateway.pending.generation,emulator.settings_flash.erases,emulator.settings_flash.programs);
    hex(emulator.gateway_descriptor,emulator.descriptor_length);
    printf("\",\"moduleReportHex\":\""); hex(emulator.report,emulator.runtime.layout.length);
    printf("\",\"gatewayReports\":["); bool first=true;
    for(unsigned slot=0;slot<OSG_GW_MAX_MODULES;slot++) {
        uint8_t bytes[OSG_GW_MAX_REPORT_SIZE]; size_t size=osg_gateway_report(&emulator.gateway,slot,bytes,sizeof bytes);
        if(!size) continue;
        printf("%s{\"reportId\":%u,\"hex\":\"",first?"":",",slot+1); hex(bytes,size); printf("\"}"); first=false;
    }
    printf("],\"module\":%s}}\n",emulator.active_config); fflush(stdout);
}
static void admin(const char *line) {
    unsigned id=0,n=0,a=0,b=0; bool ok=true; const char *error=NULL;
    if(sscanf(line,"emulator %u %n",&id,&n)!=1 || !n) { inspect(id,false,"invalid_request"); return; }
    const char *command=line+n;
    if(!strcmp(command,"inspect")) {}
    else if(!strncmp(command,"direct_config ",14)) {
        bool apply=false;error=osg_runtime_command(&emulator.runtime,command+14,emulator.now,&apply);
        ok=error==NULL;if(apply) advance(400);
    }
    else if(!strncmp(command,"probe_config ",13)) { ok=probe_config(command+13);if(!ok) error="invalid_request"; }
    else if(!strcmp(command,"reboot module")) { emulator.queued=0; boot_module(); advance(600); }
    else if(!strcmp(command,"reboot gateway")) { emulator.queued=0; boot_gateway(); osg_can_client_reset(&emulator.module); advance(600); }
    else if(sscanf(command,"fault module %u",&a)==1) emulator.settings_flash.fault=(int)a;
    else if(sscanf(command,"fault gateway %u",&a)==1) emulator.registry_flash.fault=(int)a;
    else if(sscanf(command,"advance %u",&a)==1 && a<=60000) advance(a);
    else if(sscanf(command,"axis %u %u",&a,&b)==2 && a<8 && b<=4095) { emulator.sample.axes[a]=(uint16_t)b; advance(30); }
    else if(sscanf(command,"contact %u %u",&a,&b)==2 && a<OSG_INPUTS && b<=1) {
        if(a<24) { uint32_t bit=UINT32_C(1)<<a; if(b) emulator.sample.digital_closed|=bit; else emulator.sample.digital_closed&=~bit; }
        else { unsigned bank=(a-24)/16,pin=(a-24)%16; if(b) emulator.sample.expansion_closed[bank]|=(uint16_t)(1U<<pin); else emulator.sample.expansion_closed[bank]&=(uint16_t)~(1U<<pin); }
        advance(30);
    } else if(sscanf(command,"direct %u",&a)==1 && a<=1) {
        emulator.direct=a!=0; osg_can_client_usb(&emulator.module,emulator.direct); rearm(); advance(600);
    } else if(sscanf(command,"disconnect %u",&a)==1 && a<=1) {
        emulator.disconnected=a!=0; osg_gateway_set_running(&emulator.gateway,!emulator.disconnected && !emulator.layout.detached); advance(30);
    } else if(sscanf(command,"drop_reply %u",&a)==1 && a<=1) emulator.drop_reply=a!=0;
    else if(!strcmp(command,"capture_input") && emulator.old_input_valid) {
        emulator.captured_input=emulator.old_input; emulator.captured_input_valid=true;
    } else if(!strcmp(command,"capture_delivery")) {
        emulator.captured_packet[0]=1;
        emulator.captured_length=osg_gateway_delivery_prepare(&emulator.gateway,0,emulator.captured_packet+1,OSG_GW_MAX_REPORT_SIZE,&emulator.captured_delivery);
    } else if(!strcmp(command,"complete_delivery")) {
        osg_gateway_message_t ack;
        if(!osg_gateway_delivery_complete(&emulator.gateway,&emulator.captured_delivery,emulator.captured_packet,emulator.captured_length+1,&ack)) emulator.stale_rejected++;
    } else if(!strcmp(command,"stale_input") && (emulator.captured_input_valid || emulator.old_input_valid)) {
        osg_gateway_message_t reply; const osg_gateway_message_t *old=emulator.captured_input_valid ? &emulator.captured_input : &emulator.old_input;
        if(osg_gateway_receive(&emulator.gateway,old,emulator.now,&reply)==OSG_GW_RX_REJECTED) emulator.stale_rejected++;
    } else { ok=false; error="invalid_request"; }
    inspect(id,ok,error);
}
static void command(const char *line) {
    advance(20);
    if(!strncmp(line,"emulator ",9)) { admin(line); return; }
    if(!strcmp(line,"identity") || !strcmp(line,"hello") || !strcmp(line,"info")) {
        printf("gatewayType=stm32-can-gateway protocol=2 canProtocol=3 firmware=0.3.0 board=native-emulator identity=%s epoch=%u maxModules=10\r\n",gateway_uid,emulator.gateway.epoch); fflush(stdout); return;
    }
    osg_gateway_cockpit_request_t request={0}; const char *error=NULL,*json=NULL; bool apply=false;
    if(!osg_gateway_cockpit_parse(line,&request)) error="invalid_request";
    else if(request.action==OSG_GW_COCKPIT_CONFIG) {
        error=osg_gateway_config_start(&emulator.config,&emulator.gateway,&request,emulator.now);
        if(!error) {
            while(emulator.config.busy && !emulator.config.complete) step();
            assert(osg_gateway_config_result(&emulator.config,&error,&json));
        }
    } else error=osg_gateway_cockpit_execute(&emulator.gateway,&request,persist,NULL,&apply);
    if(apply) osg_gateway_layout_schedule(&emulator.layout,emulator.now);
    size_t n=osg_gateway_cockpit_config_response(&emulator.gateway,gateway_uid,request.id,error,json,emulator.response,sizeof emulator.response);
    if(n) fputs(emulator.response,stdout);
    else printf("{\"id\":%u,\"ok\":false,\"error\":\"response_overflow\",\"state\":null}\n",request.id);
    fflush(stdout); advance(400);
}
int main(void) {
    state_directory=getenv("OSG_NATIVE_STATE_DIR");
    if(state_directory && *state_directory) { (void)mkdir(state_directory,0700); } else state_directory=NULL;
    flash_init(&emulator.registry_flash,"registry"); flash_init(&emulator.settings_flash,"settings"); flash_init(&emulator.boot_flash,"boot");
    const char *initial_fault=getenv("OSG_NATIVE_INITIAL_SETTINGS_FAULT");
    if(initial_fault && *initial_fault) {
        char *end=NULL;unsigned long cut=strtoul(initial_fault,&end,10);
        assert(end && !*end && cut<=1000);emulator.settings_flash.fault=(int)cut;
    }
    assert(osg_gateway_identity_parse("00112233445566778899aabb47575459",&emulator.gateway_identity));
    assert(osg_gateway_identity_parse("00112233445566778899aabbccddeeff",&emulator.module_identity));
    emulator.sample.axes[0]=2048;
    boot_gateway(); boot_module(); advance(600);
    char buffer[OSG_GW_CONFIG_COMMAND_CAPACITY+128]; osg_line_t line; osg_line_init(&line,buffer,sizeof buffer);
    int byte; while((byte=getchar())!=EOF) {
        osg_line_result_t result=osg_line_feed(&line,(char)byte);
        if(result==OSG_LINE_READY) command(buffer);
        else if(result==OSG_LINE_TOO_LONG) { puts("{\"id\":0,\"ok\":false,\"error\":\"invalid_request\",\"state\":null}"); fflush(stdout); }
    }
    osg_cyphal_destroy(&emulator.module.cyphal); osg_cyphal_destroy(&emulator.gateway_can); return 0;
}
