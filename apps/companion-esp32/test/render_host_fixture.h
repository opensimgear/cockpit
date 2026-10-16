#ifndef OSG_TEST_RENDER_HOST_FIXTURE_H
#define OSG_TEST_RENDER_HOST_FIXTURE_H
/* Reuse only SDK-free flash/SHA/fixture helpers; the engine suite runs separately. */
#define main engine_fixture_main
#include "engine_test.c"
#undef main
#include "../network/render_delivery.h"

static bool base64_decode(const char *text,unsigned char *out,size_t cap,size_t *written){size_t n=strlen(text);if(!n||n>2732||n%4)return false;unsigned char scratch[2052];int count=EVP_DecodeBlock(scratch,(const unsigned char *)text,(int)n);if(count<0)return false;while(n&&text[n-1]=='='){count--;n--;}if(count<0||(size_t)count>cap)return false;memcpy(out,scratch,(size_t)count);*written=(size_t)count;return true;}
static bool sha_memory(const void *data,size_t n,char out[72]){digest(data,n,out);return true;}
const osg_crypto osg_platform_crypto={sha_memory,base64_decode};

typedef struct {const fixture *package;osg_engine *engine;unsigned calls,reads,keepalives;uint32_t transferred;int stage,mutation;unsigned expected_object;uint32_t expected_offset;bool corrupt;} host;
static cJSON *reply_base(const char *type){cJSON *v=cJSON_CreateObject();assert(v);cJSON_AddBoolToObject(v,"ok",true);cJSON_AddStringToObject(v,"type",type);cJSON_AddNumberToObject(v,"version",2);return v;}
static void replace_number(cJSON *v,const char *key,double number){assert(cJSON_ReplaceItemInObjectCaseSensitive(v,key,cJSON_CreateNumber(number)));}
static void replace_text(cJSON *v,const char *key,const char *text){assert(cJSON_ReplaceItemInObjectCaseSensitive(v,key,cJSON_CreateString(text)));}
static bool fake_request(void *context,cJSON *request,cJSON **reply,unsigned timeout){host *h=context;assert(timeout==3000);h->calls++;*reply=NULL;if(!request)return false;const char *type=osg_string(request,"type");assert(type&&cJSON_GetObjectItemCaseSensitive(request,"version")->valueint==2);cJSON *v=NULL;
    if(h->stage==4&&((h->mutation<=2&&!strcmp(type,"render-capabilities"))||(h->mutation==3&&!strcmp(type,"render-prepare"))||(h->mutation==4&&!strcmp(type,"render-read")))){cJSON_Delete(request);return h->mutation==2;}
    if(!strcmp(type,"render-capabilities")){
        if(h->stage==1&&h->mutation<=3){v=cJSON_CreateObject();cJSON_AddBoolToObject(v,"ok",false);cJSON_AddStringToObject(v,"error",h->mutation==1?"Unsupported protocol version.":h->mutation==2?"Unknown message type.":"network timeout");cJSON_Delete(request);*reply=v;return false;}
        v=reply_base(type);cJSON_AddNumberToObject(v,"chunkBytes",2048);cJSON *profiles=cJSON_AddArrayToObject(v,"profiles"),*profile=cJSON_CreateObject();cJSON_AddItemToArray(profiles,profile);cJSON_AddStringToObject(profile,"id",osg_engine_profile_id(h->engine));cJSON_AddStringToObject(profile,"hash",osg_engine_profile_hash(h->engine));
        if(h->stage==1){if(h->mutation==4)replace_number(v,"version",1);if(h->mutation==5)replace_number(v,"chunkBytes",2049);if(h->mutation==6)replace_text(profile,"hash",h->package->hash);if(h->mutation==7)replace_text(v,"type","render-prepared");if(h->mutation==8)assert(cJSON_ReplaceItemInObjectCaseSensitive(v,"profiles",cJSON_CreateBool(false)));}
    }else if(!strcmp(type,"render-prepare")){
        assert(!strcmp(osg_string(request,"profileId"),osg_engine_profile_id(h->engine))&&!strcmp(osg_string(request,"profileHash"),osg_engine_profile_hash(h->engine)));
        if(h->stage==2&&h->mutation==1){v=cJSON_CreateObject();cJSON_AddBoolToObject(v,"ok",false);cJSON_AddStringToObject(v,"error","No compatible compiled package is published for this profile.");cJSON_Delete(request);*reply=v;return false;}
        cJSON *manifest=osg_json_parse(h->package->manifest,h->package->manifest_bytes);assert(manifest);bool cached=osg_string(request,"cachedPackageHash")&&!strcmp(osg_string(request,"cachedPackageHash"),h->package->hash);
        v=reply_base("render-prepared");cJSON_AddStringToObject(v,"preparedId","prepared-1");cJSON_AddStringToObject(v,"packageHash",h->package->hash);cJSON_AddStringToObject(v,"profileHash",osg_engine_profile_hash(h->engine));cJSON_AddStringToObject(v,"semanticDescriptorHash",osg_string(manifest,"semanticDescriptorHash"));cJSON_AddNumberToObject(v,"chunkBytes",2048);cJSON_AddBoolToObject(v,"cached",cached);if(cached)cJSON_AddNullToObject(v,"manifest");else cJSON_AddStringToObject(v,"manifest",h->package->manifest);
        if(h->stage==2){switch(h->mutation){case 2:replace_number(v,"version",1);break;case 3:replace_number(v,"chunkBytes",1);break;case 4:replace_text(v,"preparedId","bad id");break;case 5:replace_text(v,"packageHash","bad");break;case 6:replace_text(v,"semanticDescriptorHash",h->package->hash);break;case 7:replace_text(v,"profileHash",h->package->hash);break;case 8:assert(cJSON_ReplaceItemInObjectCaseSensitive(v,"cached",cJSON_CreateNumber(1)));break;case 9:assert(cJSON_ReplaceItemInObjectCaseSensitive(v,"manifest",cJSON_CreateBool(false)));break;case 10:assert(cJSON_ReplaceItemInObjectCaseSensitive(v,"cached",cJSON_CreateBool(true)));break;case 11:assert(cJSON_ReplaceItemInObjectCaseSensitive(v,"manifest",cJSON_CreateString("{}")));break;case 12:replace_text(v,"type","session-open");break;case 13:replace_text(v,"packageHash",h->engine->source_hash);break;}}
        cJSON_Delete(manifest);
    }else{
        assert(!strcmp(type,"render-read")&&!strcmp(osg_string(request,"preparedId"),"prepared-1")&&!strcmp(osg_string(request,"packageHash"),h->package->hash));
        const char *kind=osg_string(request,"kind");uint32_t offset=(uint32_t)cJSON_GetObjectItemCaseSensitive(request,"offset")->valuedouble,wanted=(uint32_t)cJSON_GetObjectItemCaseSensitive(request,"bytes")->valuedouble;const uint8_t *data=NULL;size_t total=0;const char *names[]={"semantics","scene","images","fonts"};
        if(!strcmp(kind,"manifest")){assert(!offset&&wanted==1);data=(const uint8_t *)h->package->manifest;total=h->package->manifest_bytes;h->keepalives++;}
        else{while(h->expected_object<4&&!h->package->bytes[h->expected_object])h->expected_object++;assert(h->expected_object<4&&!strcmp(kind,names[h->expected_object])&&offset==h->expected_offset&&wanted==2048);data=h->package->objects[h->expected_object];total=h->package->bytes[h->expected_object];}
        assert(offset<total);size_t count=total-offset;if(count>wanted)count=wanted;
        unsigned char raw[2048];memcpy(raw,data+offset,count);if(h->corrupt&&strcmp(kind,"manifest")&&offset==0&&h->expected_object==0)raw[0]^=1;char encoded[2733];assert(EVP_EncodeBlock((unsigned char *)encoded,raw,(int)count)>0);
        v=reply_base("render-chunk");cJSON_AddStringToObject(v,"preparedId","prepared-1");cJSON_AddStringToObject(v,"packageHash",h->package->hash);cJSON *chunk=cJSON_AddObjectToObject(v,"chunk");cJSON_AddStringToObject(chunk,"kind",kind);cJSON_AddNumberToObject(chunk,"offset",offset);cJSON_AddNumberToObject(chunk,"bytes",count);cJSON_AddNumberToObject(chunk,"totalBytes",total);cJSON_AddStringToObject(chunk,"base64",encoded);
        if(h->stage==3){switch(h->mutation){case 1:replace_number(v,"version",1);break;case 2:replace_text(v,"preparedId","stale");break;case 3:replace_text(v,"packageHash",h->engine->source_hash);break;case 4:replace_text(chunk,"kind","wrong");break;case 5:replace_number(chunk,"offset",offset+1);break;case 6:replace_number(chunk,"bytes",count+1);break;case 7:replace_number(chunk,"totalBytes",total+1);break;case 8:replace_text(chunk,"base64","!");break;case 9:replace_text(v,"type","render-prepared");break;case 10:replace_text(chunk,"base64","AA==");break;case 11:assert(cJSON_ReplaceItemInObjectCaseSensitive(chunk,"offset",cJSON_CreateNumber(-1)));break;case 12:cJSON_DeleteItemFromObjectCaseSensitive(v,"chunk");break;}}
        if(strcmp(kind,"manifest")){h->reads++;h->transferred+=(uint32_t)count;h->expected_offset+=(uint32_t)count;if(h->expected_offset==total){h->expected_object++;h->expected_offset=0;}}
    }
    cJSON_Delete(request);*reply=v;return true;
}
#endif
