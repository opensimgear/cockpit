#include "render_feedback.h"
#include <math.h>
#include <string.h>

static const cJSON *get(const cJSON *v,const char *name){return cJSON_GetObjectItemCaseSensitive(v,name);}
static bool fields(const cJSON *v){
    if(!cJSON_IsObject(v))return false;
    const char *names[]={"v","g","s","m","x","b"};unsigned seen=0;
    for(const cJSON *item=v->child;item;item=item->next){unsigned i=0;while(i<6&&(!item->string||strcmp(item->string,names[i])))i++;if(i==6||(seen&(1u<<i)))return false;seen|=1u<<i;}
    return (seen&31u)==31u;
}
static bool integer(const cJSON *v,unsigned maximum,unsigned *out){if(!cJSON_IsNumber(v)||!isfinite(v->valuedouble)||v->valuedouble<0||v->valuedouble>maximum||floor(v->valuedouble)!=v->valuedouble)return false;*out=(unsigned)v->valuedouble;return true;}
static const osg_widget *output(const osg_panel *panel,unsigned wanted){
    unsigned index=0;for(size_t i=0;i<panel->widget_count;i++)if(panel->widgets[i].kind!=OSG_BUTTON){if(index++==wanted)return &panel->widgets[i];}return NULL;
}
static bool text(const char *value){if(!value||strlen(value)>120)return false;for(const unsigned char *p=(const unsigned char *)value;*p;p++)if(*p<32||*p==127)return false;return true;}
static bool add_string(cJSON *v,const char *key,const char *value){return value&&cJSON_AddStringToObject(v,key,value);}
static bool add_number(cJSON *v,const char *key,double value){return cJSON_AddNumberToObject(v,key,value)!=NULL;}
static bool append(cJSON *list,cJSON *item){if(!item)return false;if(!cJSON_AddItemToArray(list,item)){cJSON_Delete(item);return false;}return true;}
static bool sample(cJSON *values,const cJSON *row,const osg_widget *widget){
    const unsigned count=(unsigned)cJSON_GetArraySize(row);unsigned status,provenance,source,age;
    if(!cJSON_IsArray(row)||(count!=5&&count!=6)||!integer(cJSON_GetArrayItem(row,1),3,&status)||!integer(cJSON_GetArrayItem(row,2),4,&provenance)||!integer(cJSON_GetArrayItem(row,3),2,&source)||!integer(cJSON_GetArrayItem(row,4),86400000,&age)||
       (provenance==4&&source!=1)||(status==0&&count!=6)||((status==1||status==3)&&count!=5))return false;
    const bool numeric=widget->kind==OSG_NUMERIC||widget->kind==OSG_GAUGE;
    if(!numeric&&widget->kind!=OSG_INDICATOR&&widget->kind!=OSG_TEXT)return false;
    const cJSON *value=cJSON_GetArrayItem(row,5);
    if(count==6){
        if(numeric){if(!cJSON_IsNumber(value)||!isfinite(value->valuedouble)||value->valuedouble<widget->min||value->valuedouble>widget->max)return false;}
        else if(widget->kind==OSG_INDICATOR){if(!cJSON_IsBool(value))return false;}
        else if(!cJSON_IsString(value)||!text(value->valuestring))return false;
    }
    const char *statuses[]={"fresh","unknown","stale","disconnected"},*provenances[]={"requested","local","host-accepted","host-submitted","game-confirmed"},*sources[]={"host","simulator","script"};
    cJSON *item=cJSON_CreateObject();if(!item)return false;
    const char *unit=numeric?(widget->unit&&*widget->unit?widget->unit:"ratio"):widget->kind==OSG_TEXT?"text":"state";
    bool ok=add_string(item,"channelId",widget->reference)&&add_string(item,"outputId",widget->output_id)&&add_string(item,"status",statuses[status])&&add_string(item,"provenance",provenances[provenance])&&add_string(item,"sourceKind",sources[source])&&add_number(item,"sourceAgeMs",age)&&add_string(item,"unit",unit);
    if(ok&&numeric){cJSON *range=cJSON_CreateObject();ok=range&&add_number(range,"min",widget->min)&&add_number(range,"max",widget->max);if(ok)ok=cJSON_AddItemToObject(item,"range",range);if(!ok)cJSON_Delete(range);}
    if(ok&&count==6){cJSON *copy=cJSON_Duplicate(value,true);ok=copy&&cJSON_AddItemToObject(item,"value",copy);if(!ok)cJSON_Delete(copy);}
    if(ok)return append(values,item);
    cJSON_Delete(item);return false;
}
bool osg_render_feedback_ack(const cJSON *ack,osg_live_session *session,const char *sequence,osg_runtime *runtime,uint64_t now,size_t max_ack_bytes){
    if(!ack||!session||!sequence||!runtime||!runtime->panel||!max_ack_bytes||max_ack_bytes>4096||get(ack,"feedback")||get(ack,"feedbackSequence"))return false;
    const cJSON *compact=get(ack,"renderFeedback");if(!fields(compact))return false;
    unsigned version,mode;if(!integer(get(compact,"v"),2,&version)||version!=2||!integer(get(compact,"m"),1,&mode))return false;
    uint64_t counter;if(!osg_counter(osg_string(compact,"g"),&counter)||!osg_counter(osg_string(compact,"s"),&counter))return false;
    const cJSON *bindings=get(compact,"b"),*rows=get(compact,"x");
    if(!cJSON_IsArray(rows)||cJSON_GetArraySize(rows)>OSG_MAX_WIDGETS||(mode==0?!cJSON_IsArray(bindings):bindings!=NULL))return false;
    if(mode==0&&(cJSON_GetArraySize(bindings)>OSG_MAX_WIDGETS||cJSON_GetArraySize(bindings)!=cJSON_GetArraySize(rows)))return false;
    char *wire=cJSON_PrintUnformatted(ack);if(!wire)return false;const bool fits=strlen(wire)<=max_ack_bytes;cJSON_free(wire);if(!fits)return false;
    cJSON *expanded=cJSON_Duplicate(ack,true),*frame=cJSON_CreateObject(),*values=cJSON_CreateArray(),*bound=mode==0?cJSON_CreateArray():NULL;
    bool ok=expanded&&frame&&values&&(mode!=0||bound);
    if(ok)ok=add_number(frame,"version",1)&&add_string(frame,"type","feedback")&&add_string(frame,"clientId",session->client)&&add_string(frame,"sessionId",session->session)&&add_string(frame,"generation",session->generation)&&add_string(frame,"publicationHash",session->publication)&&add_string(frame,"controllerInstanceId",session->instance)&&add_string(frame,"sequence",osg_string(compact,"s"))&&add_string(frame,"bindingGeneration",osg_string(compact,"g"))&&add_string(frame,"mode",mode==0?"snapshot":"delta");
    bool seen[OSG_MAX_WIDGETS]={false};
    for(const cJSON *entry=bindings?bindings->child:NULL;ok&&entry;entry=entry->next){
        unsigned index;const osg_widget *widget=NULL;if(!integer(entry,OSG_MAX_WIDGETS-1,&index)||seen[index]||!(widget=output(runtime->panel,index))){ok=false;break;}seen[index]=true;
        cJSON *binding=cJSON_CreateObject();if(!binding){ok=false;break;}ok=add_string(binding,"channelId",widget->reference)&&add_string(binding,"outputId",widget->output_id);if(ok)ok=append(bound,binding);else cJSON_Delete(binding);
    }
    memset(seen,0,sizeof(seen));
    for(const cJSON *row=rows->child;ok&&row;row=row->next){
        unsigned index;const osg_widget *widget=NULL;if(!cJSON_IsArray(row)||!integer(cJSON_GetArrayItem(row,0),OSG_MAX_WIDGETS-1,&index)||seen[index]||!(widget=output(runtime->panel,index))){ok=false;break;}seen[index]=true;ok=sample(values,row,widget);
    }
    if(ok){ok=cJSON_AddItemToObject(frame,"values",values);if(ok)values=NULL;}
    if(ok&&bound){ok=cJSON_AddItemToObject(frame,"bindings",bound);if(ok)bound=NULL;}
    if(ok){cJSON_DeleteItemFromObjectCaseSensitive(expanded,"renderFeedback");ok=cJSON_AddItemToObject(expanded,"feedback",frame);if(ok)frame=NULL;}
    /* Existing validator performs identity, provenance, binding-generation,
     * sequence, full-snapshot, duplicate-channel and atomic commit checks. */
    if(ok)ok=osg_ack(expanded,session,sequence,runtime,now);
    cJSON_Delete(expanded);cJSON_Delete(frame);cJSON_Delete(values);cJSON_Delete(bound);return ok;
}
