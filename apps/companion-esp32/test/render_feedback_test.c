#include "render_feedback.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static const char hash[]="sha256:1111111111111111111111111111111111111111111111111111111111111111";
static const osg_page pages[]={{"main","Main",480,320}};
#define WIDGET(id_,ref_,out_,kind_,unit_) {.id=id_,.reference=ref_,.output_id=out_,.label="Value",.prefix="",.suffix="",.unit=unit_,.kind=kind_,.bounds={0,.2,.2,.2},.max=100}
static const osg_widget widgets[]={
    {.id="button",.reference="laser",.label="Laser",.prefix="",.suffix="",.unit="",.kind=OSG_BUTTON,.bounds={0,0,.1,.1}},
    WIDGET("power","power-channel","power-readout",OSG_NUMERIC,"%"),
    WIDGET("gauge","power-channel","power-needle",OSG_GAUGE,"%"),
    WIDGET("state","state-channel","laser-state",OSG_INDICATOR,""),
    WIDGET("text","text-channel","message",OSG_TEXT,"")
};
static const osg_panel panel={"panel",hash,pages,widgets,1,5};
static void fill(void *c,int x,int y,int w,int h,uint16_t color){(void)c;(void)x;(void)y;(void)w;(void)h;(void)color;}
static void draw_text(void *c,int x,int y,int w,int h,const char *text,uint16_t color){(void)c;(void)x;(void)y;(void)w;(void)h;(void)text;(void)color;}
static cJSON *parse(const char *s){cJSON *v=osg_json_parse(s,strlen(s));assert(v);return v;}
static cJSON *get(cJSON *v,const char *key){return cJSON_GetObjectItemCaseSensitive(v,key);}
static cJSON *row(cJSON *v,unsigned n){return cJSON_GetArrayItem(get(get(v,"renderFeedback"),"x"),(int)n);}
static void str(cJSON *v,const char *key,const char *s){assert(cJSON_ReplaceItemInObjectCaseSensitive(v,key,cJSON_CreateString(s)));}
static void set(cJSON *v,int n,const char *json){assert(cJSON_ReplaceItemInArray(v,n,parse(json)));}
static cJSON *ack(void){
    cJSON *v=parse("{\"ok\":true,\"type\":\"ack\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"sequence\":\"7\",\"ackStage\":\"source-accepted\",\"submitted\":false,\"renderFeedback\":{\"v\":2,\"g\":\"1\",\"s\":\"1\",\"m\":0,\"b\":[0,2],\"x\":[[0,0,4,1,3,37.125],[2,0,2,0,10,true]]}}");
    assert(cJSON_AddStringToObject(v,"publicationHash",hash));return v;
}
static void initialize(osg_runtime *runtime,osg_live_session *session){
    assert(osg_init(runtime,&panel,(osg_display){NULL,480,320,fill,draw_text},NULL,NULL));osg_connected(runtime,true);memset(session,0,sizeof(*session));
    assert(osg_copy(session->client,sizeof(session->client),"client"));assert(osg_copy(session->session,sizeof(session->session),"session"));
    assert(osg_copy(session->generation,sizeof(session->generation),"1"));assert(osg_copy(session->publication,sizeof(session->publication),hash));assert(osg_copy(session->instance,sizeof(session->instance),"instance"));
}
static void reject(cJSON *v,osg_runtime *runtime,osg_live_session *session,size_t budget){
    osg_runtime before=*runtime;osg_live_session previous=*session;assert(!osg_render_feedback_ack(v,session,"7",runtime,900,budget));
    assert(!memcmp(&before,runtime,sizeof(before)));assert(!memcmp(&previous,session,sizeof(previous)));
}
static void valid_and_replay(void){
    osg_runtime r;osg_live_session s;initialize(&r,&s);cJSON *v=ack();assert(osg_render_feedback_ack(v,&s,"7",&r,100,3072));
    assert(s.feedback_envelope&&s.binding_count==2&&s.feedback_sequence==1);assert(r.samples[1].available&&r.samples[1].number==37.125&&r.samples[1].provenance==OSG_GAME_CONFIRMED&&r.samples[1].source_age_ms==3);
    assert(!r.samples[2].available&&r.samples[3].boolean);assert(osg_render_feedback_ack(v,&s,"7",&r,999,512));assert(r.samples[1].received_ms==100);
    cJSON *c=get(v,"renderFeedback");cJSON_DeleteItemFromObjectCaseSensitive(c,"b");assert(cJSON_ReplaceItemInObjectCaseSensitive(c,"m",cJSON_CreateNumber(1)));str(c,"s","2");
    set(row(v,0),5,"50");assert(osg_render_feedback_ack(v,&s,"7",&r,200,512));set(row(v,0),5,"75");assert(osg_render_feedback_ack(v,&s,"7",&r,500,512));assert(r.samples[1].number==50&&r.samples[1].received_ms==200);
    str(c,"s","1");reject(v,&r,&s,512);str(c,"s","3");str(c,"g","2");reject(v,&r,&s,512);cJSON_Delete(v);
    v=ack();c=get(v,"renderFeedback");str(c,"g","2");str(c,"s","0");cJSON_DeleteItemFromArray(row(v,0),5);set(row(v,0),1,"1");cJSON_DeleteItemFromArray(row(v,1),5);set(row(v,1),1,"3");
    assert(osg_render_feedback_ack(v,&s,"7",&r,600,512));assert(!r.samples[1].available&&!r.samples[3].available);cJSON_Delete(v);
    for(unsigned status=0;status<4;status++)for(unsigned provenance=0;provenance<5;provenance++){
        initialize(&r,&s);v=ack();c=get(v,"renderFeedback");cJSON_DeleteItemFromArray(get(c,"b"),1);cJSON_DeleteItemFromArray(get(c,"x"),1);
        assert(cJSON_ReplaceItemInArray(row(v,0),1,cJSON_CreateNumber(status)));assert(cJSON_ReplaceItemInArray(row(v,0),2,cJSON_CreateNumber(provenance)));assert(cJSON_ReplaceItemInArray(row(v,0),3,cJSON_CreateNumber(provenance==4?1:2)));
        if(status==1||status==3)cJSON_DeleteItemFromArray(row(v,0),5);
        assert(osg_render_feedback_ack(v,&s,"7",&r,700,512));assert(r.samples[1].available==(status==0||status==2));cJSON_Delete(v);
    }
}
static void malformed(void){
    for(unsigned fault=0;fault<49;fault++){
        osg_runtime r;osg_live_session s;initialize(&r,&s);cJSON *v=ack(),*c=get(v,"renderFeedback");
        switch(fault){
            case 0:cJSON_AddNumberToObject(c,"extra",0);break;
            case 1:cJSON_AddNumberToObject(c,"v",2);break;
            case 2:cJSON_DeleteItemFromObjectCaseSensitive(c,"g");break;
            case 3:str(c,"g","01");break;
            case 4:str(c,"s","18446744073709551616");break;
            case 5:cJSON_ReplaceItemInObjectCaseSensitive(c,"v",cJSON_CreateNumber(1));break;
            case 6:cJSON_ReplaceItemInObjectCaseSensitive(c,"m",cJSON_CreateNumber(2));break;
            case 7:cJSON_ReplaceItemInObjectCaseSensitive(c,"x",cJSON_CreateObject());break;
            case 8:cJSON_DeleteItemFromObjectCaseSensitive(c,"b");break;
            case 9:cJSON_ReplaceItemInObjectCaseSensitive(c,"m",cJSON_CreateNumber(1));break;
            case 10:cJSON_DeleteItemFromArray(get(c,"x"),1);break;
            case 11:set(get(c,"b"),0,"32");break;
            case 12:set(get(c,"b"),1,"0");break;
            case 13:set(get(c,"b"),0,"4");break;
            case 14:set(get(c,"b"),0,"0.5");break;
            case 15:set(row(v,0),0,"32");break;
            case 16:set(row(v,1),0,"0");break;
            case 17:set(row(v,0),0,"4");break;
            case 18:set(row(v,0),1,"4");break;
            case 19:set(row(v,0),2,"5");break;
            case 20:set(row(v,0),3,"3");break;
            case 21:set(row(v,0),4,"86400001");break;
            case 22:set(row(v,0),3,"0");break;
            case 23:cJSON_DeleteItemFromArray(row(v,0),5);break;
            case 24:set(row(v,0),1,"1");break;
            case 25:set(row(v,0),1,"3");break;
            case 26:set(row(v,0),5,"true");break;
            case 27:set(row(v,0),5,"101");break;
            case 28:set(row(v,1),5,"1");break;
            case 29:cJSON_AddItemToArray(row(v,0),cJSON_CreateNumber(1));break;
            case 30:set(row(v,0),4,"-1");break;
            case 31:set(row(v,0),4,"0.25");break;
            case 32:set(get(c,"x"),0,"0");break;
            case 33:str(v,"clientId","other");break;
            case 34:str(v,"sessionId","other");break;
            case 35:str(v,"generation","2");break;
            case 36:str(v,"sequence","8");break;
            case 37:str(v,"ackStage","game-confirmed");break;
            case 38:str(v,"publicationHash","wrong");break;
            case 39:cJSON_AddNullToObject(v,"feedback");break;
            case 40:cJSON_AddNullToObject(v,"feedbackSequence");break;
            case 41:{char p[600];memset(p,'x',sizeof(p)-1);p[sizeof(p)-1]=0;cJSON_AddStringToObject(v,"padding",p);break;}
            case 42:set(row(v,0),5,"-0.1");break;
            case 43:set(row(v,0),0,"\"0\"");break;
            case 44:cJSON_ReplaceItemInObjectCaseSensitive(c,"s",cJSON_CreateNumber(1));break;
            case 45:cJSON_ReplaceItemInObjectCaseSensitive(v,"renderFeedback",cJSON_CreateNull());break;
            case 46:cJSON_ReplaceItemInObjectCaseSensitive(c,"b",cJSON_CreateObject());break;
            case 47:cJSON_ReplaceItemInObjectCaseSensitive(v,"submitted",cJSON_CreateBool(true));break;
            case 48:cJSON_ReplaceItemInObjectCaseSensitive(v,"ok",cJSON_CreateBool(false));break;
        }
        reject(v,&r,&s,512);cJSON_Delete(v);
    }
    osg_runtime r;osg_live_session s;initialize(&r,&s);cJSON *v=ack();reject(v,&r,&s,0);reject(v,&r,&s,4097);
    assert(!osg_render_feedback_ack(NULL,&s,"7",&r,0,512));assert(!osg_render_feedback_ack(v,NULL,"7",&r,0,512));assert(!osg_render_feedback_ack(v,&s,NULL,&r,0,512));assert(!osg_render_feedback_ack(v,&s,"7",NULL,0,512));
    r.panel=NULL;assert(!osg_render_feedback_ack(v,&s,"7",&r,0,512));cJSON_Delete(v);
}
static void text_cases(void){
    for(unsigned fault=0;fault<5;fault++){
        osg_runtime r;osg_live_session s;initialize(&r,&s);cJSON *v=ack(),*c=get(v,"renderFeedback");cJSON_ReplaceItemInObjectCaseSensitive(c,"b",parse("[3]"));cJSON_ReplaceItemInObjectCaseSensitive(c,"x",parse("[[3,0,1,2,0,\"ready\"]]"));
        char value[122];memset(value,'a',sizeof(value)-1);value[121]=0;if(fault==0)value[120]=0;if(fault==1)value[0]='\n';if(fault==2)value[0]=127;
        if(fault==3)cJSON_ReplaceItemInArray(row(v,0),5,cJSON_CreateBool(true));else cJSON_ReplaceItemInArray(row(v,0),5,cJSON_CreateString(value));
        if(fault==0){assert(osg_render_feedback_ack(v,&s,"7",&r,0,512));assert(strlen(r.samples[4].text)==120);}else reject(v,&r,&s,512);cJSON_Delete(v);
    }
}
int main(void){osg_json_init();valid_and_replay();malformed();text_cases();assert(!osg_json_allocated());puts("Compact renderer ACK roundtrip, replay, full snapshots, bounds and atomic rejection passed");return 0;}