#include "render_delivery.h"
#include <math.h>
#include <string.h>
extern const osg_crypto osg_platform_crypto;
static bool same(const char *a,const char *b){return a&&b&&!strcmp(a,b);}
static cJSON *message(const char *type){cJSON *v=cJSON_CreateObject();if(v){cJSON_AddStringToObject(v,"type",type);cJSON_AddNumberToObject(v,"version",2);}return v;}
static bool number(const cJSON *v,const char *key,uint32_t *out){const cJSON *n=cJSON_GetObjectItemCaseSensitive(v,key);if(!cJSON_IsNumber(n)||!isfinite(n->valuedouble)||n->valuedouble<0||n->valuedouble>UINT32_MAX||floor(n->valuedouble)!=n->valuedouble)return false;*out=(uint32_t)n->valuedouble;return true;}
static osg_delivery_phase fail(osg_render_delivery *d,const char *why){d->error=why;return d->phase=OSG_DELIVERY_ERROR;}
static cJSON *read_request(const osg_render_delivery *d,const char *kind,uint32_t offset,uint32_t bytes){cJSON *v=message("render-read");if(v){cJSON_AddStringToObject(v,"preparedId",d->prepared);cJSON_AddStringToObject(v,"packageHash",d->package);cJSON_AddStringToObject(v,"kind",kind);cJSON_AddNumberToObject(v,"offset",offset);cJSON_AddNumberToObject(v,"bytes",bytes);}return v;}
static bool identity(const cJSON *r,const osg_render_delivery *d){uint32_t version;return cJSON_IsTrue(cJSON_GetObjectItemCaseSensitive(r,"ok"))&&number(r,"version",&version)&&version==2&&same(osg_string(r,"type"),"render-chunk")&&same(osg_string(r,"preparedId"),d->prepared)&&same(osg_string(r,"packageHash"),d->package);}
bool osg_delivery_start(osg_render_delivery *d,const osg_engine *e,const char *instance,const char *publication){
    if(!d||!e||!osg_stable_id(instance)||!osg_hash_id(publication))return false;
    memset(d,0,sizeof(*d));d->phase=e->available?OSG_DELIVERY_NEGOTIATE:OSG_DELIVERY_LEGACY;
    return osg_copy(d->instance,sizeof(d->instance),instance)&&osg_copy(d->publication,sizeof(d->publication),publication);
}
osg_delivery_phase osg_delivery_step(osg_render_delivery *d,osg_engine *e,osg_delivery_request request,void *context,uint64_t now,bool suspended){
    if(!d||!e||!request)return OSG_DELIVERY_ERROR;
    if(d->phase>=OSG_DELIVERY_COMPLETE)return d->phase;
    if(!suspended)return fail(d,"render-main-not-suspended");
    cJSON *v=NULL,*reply=NULL;bool ok=false;
    if(d->phase==OSG_DELIVERY_NEGOTIATE){
        ok=request(context,message("render-capabilities"),&reply,3000);
        if(!ok){
            const char *error=osg_string(reply,"error");
            if(same(error,"Unsupported protocol version.")||same(error,"Unknown message type.")){d->phase=OSG_DELIVERY_LEGACY;cJSON_Delete(reply);return d->phase;}
        }else{
            uint32_t version,chunk;const cJSON *profiles=cJSON_GetObjectItemCaseSensitive(reply,"profiles");bool matched=false;
            ok=same(osg_string(reply,"type"),"render-capabilities")&&number(reply,"version",&version)&&version==2&&number(reply,"chunkBytes",&chunk)&&chunk==2048&&cJSON_IsArray(profiles)&&cJSON_GetArraySize(profiles)<=8;
            for(const cJSON *p=profiles?profiles->child:NULL;ok&&p;p=p->next)if(same(osg_string(p,"id"),osg_engine_profile_id(e))&&same(osg_string(p,"hash"),osg_engine_profile_hash(e)))matched=true;
            ok=ok&&matched;if(ok)d->phase=OSG_DELIVERY_PREPARE;
        }
    }else if(d->phase==OSG_DELIVERY_PREPARE){
        v=message("render-prepare");cJSON_AddStringToObject(v,"instanceId",d->instance);cJSON_AddStringToObject(v,"publicationHash",d->publication);
        cJSON_AddStringToObject(v,"profileId",osg_engine_profile_id(e));cJSON_AddStringToObject(v,"profileHash",osg_engine_profile_hash(e));
        const char *cached=osg_engine_current_package(e);if(cached&&*cached)cJSON_AddStringToObject(v,"cachedPackageHash",cached);
        ok=request(context,v,&reply,3000);
        if(!ok&&same(osg_string(reply,"error"),"No compatible compiled package is published for this profile.")){d->phase=OSG_DELIVERY_LEGACY;cJSON_Delete(reply);return d->phase;}
        uint32_t version,chunk;
        ok=ok&&same(osg_string(reply,"type"),"render-prepared")&&number(reply,"version",&version)&&version==2&&number(reply,"chunkBytes",&chunk)&&chunk==2048&&same(osg_string(reply,"profileHash"),osg_engine_profile_hash(e))&&
            osg_stable_id(osg_string(reply,"preparedId"))&&osg_hash_id(osg_string(reply,"packageHash"))&&osg_hash_id(osg_string(reply,"semanticDescriptorHash"));
        if(ok){
            ok=osg_copy(d->prepared,sizeof(d->prepared),osg_string(reply,"preparedId"))&&osg_copy(d->package,sizeof(d->package),osg_string(reply,"packageHash"))&&osg_copy(d->semantic,sizeof(d->semantic),osg_string(reply,"semanticDescriptorHash"));
            const cJSON *warm=cJSON_GetObjectItemCaseSensitive(reply,"cached"),*manifest=cJSON_GetObjectItemCaseSensitive(reply,"manifest");
            if(cJSON_IsTrue(warm)){
                ok=ok&&cJSON_IsNull(manifest)&&same(cached,d->package)&&same(e->source_hash,d->publication)&&same(e->semantic_hash,d->semantic);
                if(ok)d->phase=OSG_DELIVERY_COMPLETE;
            }else if(cJSON_IsFalse(warm)&&cJSON_IsString(manifest)&&strlen(manifest->valuestring)<=OSG_ENGINE_MANIFEST_BYTES){
                cJSON *m=osg_json_parse(manifest->valuestring,strlen(manifest->valuestring));
                ok=ok&&m&&same(osg_string(m,"sourcePublicationHash"),d->publication)&&same(osg_string(m,"semanticDescriptorHash"),d->semantic);
                if(ok)ok=osg_engine_begin(e,manifest->valuestring,strlen(manifest->valuestring),d->package,true)==OSG_CONTENT_OK;
                cJSON_Delete(m);
                if(ok){for(unsigned i=0;i<4;i++)d->total+=e->pending.objects[i].bytes;d->phase=OSG_DELIVERY_ERASE;d->keepalive_ms=now;}
            }else ok=false;
        }
    }else if(d->phase==OSG_DELIVERY_ERASE){
        if(now<d->keepalive_ms||now-d->keepalive_ms>=1000){
            ok=request(context,read_request(d,"manifest",0,1),&reply,3000)&&identity(reply,d);d->keepalive_ms=now;
        }else{bool done=false;ok=osg_engine_erase_step(e,&done)==OSG_CONTENT_OK;if(ok&&done)d->phase=OSG_DELIVERY_READ;}
    }else if(d->phase==OSG_DELIVERY_READ){
        while(d->object<4&&!e->pending.objects[d->object].bytes)d->object++;
        if(d->object==4){d->phase=OSG_DELIVERY_VERIFY;return d->phase;}
        const char *kinds[]={"semantics","scene","images","fonts"};const uint32_t size=e->pending.objects[d->object].bytes,remain=size-d->offset,wanted=remain>2048?2048:remain;
        ok=request(context,read_request(d,kinds[d->object],d->offset,2048),&reply,3000)&&identity(reply,d);
        const cJSON *chunk=cJSON_GetObjectItemCaseSensitive(reply,"chunk");uint32_t offset,bytes,total;unsigned char decoded[2048];size_t count=0;
        const char *encoded=osg_string(chunk,"base64");
        ok=ok&&same(osg_string(chunk,"kind"),kinds[d->object])&&number(chunk,"offset",&offset)&&offset==d->offset&&number(chunk,"bytes",&bytes)&&bytes==wanted&&number(chunk,"totalBytes",&total)&&total==size&&encoded&&strlen(encoded)<=2732;
        if(ok)ok=osg_platform_crypto.base64(encoded,decoded,sizeof(decoded),&count)&&count==wanted&&osg_engine_write(e,d->object,d->offset,decoded,count)==OSG_CONTENT_OK;
        if(ok){d->offset+=wanted;d->received+=wanted;if(d->offset==size){d->object++;d->offset=0;}}
    }else if(d->phase==OSG_DELIVERY_VERIFY){
        ok=osg_engine_finish(e,true)==OSG_CONTENT_OK&&same(osg_engine_current_package(e),d->package)&&same(e->source_hash,d->publication)&&same(e->semantic_hash,d->semantic);
        if(ok)d->phase=OSG_DELIVERY_COMPLETE;
    }
    cJSON_Delete(reply);return ok?d->phase:fail(d,"render-package-transfer-or-validation-failed");
}
cJSON *osg_delivery_open_request(const osg_render_delivery *d,const osg_engine *e){
    if(!d||!e||d->phase!=OSG_DELIVERY_COMPLETE||!same(osg_engine_current_package(e),d->package))return NULL;
    cJSON *v=message("render-open");if(v){cJSON_AddStringToObject(v,"instanceId",d->instance);cJSON_AddStringToObject(v,"publicationHash",d->publication);cJSON_AddStringToObject(v,"preparedId",d->prepared);cJSON_AddStringToObject(v,"packageHash",d->package);cJSON_AddStringToObject(v,"profileHash",osg_engine_profile_hash(e));cJSON_AddStringToObject(v,"semanticDescriptorHash",d->semantic);}return v;
}
