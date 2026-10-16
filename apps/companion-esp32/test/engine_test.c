#include "../render/engine.h"
#include <assert.h>
#include <openssl/evp.h>
#include <openssl/sha.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
static uint8_t flash_bytes[0x600000];static size_t flash_size;static bool fail_commit;static unsigned pixel_writes,flash_writes;
static EVP_MD_CTX *sha;static unsigned sha_failure;static bool read_failure;
typedef struct {char *manifest;size_t manifest_bytes;uint8_t *objects[4];size_t bytes[4];char hash[72];} fixture;
static bool read_flash(void *c,uint32_t offset,void *out,size_t n){(void)c;assert(offset<=flash_size&&n<=flash_size-offset&&n<=2048);if(read_failure)return false;memcpy(out,flash_bytes+offset,n);return true;}
static bool write_flash(void *c,uint32_t offset,const void *p,size_t n){(void)c;assert(offset<=flash_size&&n<=flash_size-offset&&n<=2048);if(fail_commit&&offset<8192)return false;const uint8_t *v=p;for(size_t i=0;i<n;i++){assert((flash_bytes[offset+i]&v[i])==v[i]);flash_bytes[offset+i]&=v[i];}flash_writes++;return true;}
static bool erase_flash(void *c,uint32_t offset,uint32_t n){(void)c;assert(offset%4096==0&&n==4096&&offset+n<=flash_size);memset(flash_bytes+offset,255,n);return true;}
static bool sha_begin(void *c){return sha_failure!=1&&EVP_DigestInit_ex(c,EVP_sha256(),NULL)==1;}
static bool sha_update(void *c,const void *p,size_t n){assert(n<=2048);return sha_failure!=2&&EVP_DigestUpdate(c,p,n)==1;}
static bool sha_finish(void *c,uint8_t out[32]){unsigned n;return sha_failure!=3&&EVP_DigestFinal_ex(c,out,&n)==1&&n==32;}
static osg_content_io io(void){return (osg_content_io){.bytes=(uint32_t)flash_size,.erase_bytes=4096,.read=read_flash,.write=write_flash,.erase=erase_flash,.hash_context=sha,.hash_begin=sha_begin,.hash_update=sha_update,.hash_finish=sha_finish};}
static void digest(const void *p,size_t n,char out[72]){unsigned char bytes[32];SHA256(p,n,bytes);memcpy(out,"sha256:",7);for(unsigned i=0;i<32;i++)sprintf(out+7+i*2,"%02x",bytes[i]);}
static void *load(const char *path,size_t *n){FILE *f=fopen(path,"rb");assert(f);assert(fseek(f,0,SEEK_END)==0);long size=ftell(f);assert(size>0&&size<4*1024*1024);rewind(f);char *p=malloc((size_t)size+1);assert(p&&fread(p,1,(size_t)size,f)==(size_t)size);fclose(f);p[size]=0;*n=(size_t)size;return p;}
static fixture fixture_load(const char *base,const char *board,const char *revision){fixture f={0};char path[1024];snprintf(path,sizeof(path),"%s/%s/%s/manifest.json",base,board,revision);f.manifest=load(path,&f.manifest_bytes);digest(f.manifest,f.manifest_bytes,f.hash);
    const char *names[]={"semantics","scene","images","fonts"};for(unsigned i=0;i<4;i++){snprintf(path,sizeof(path),"%s/%s/%s/%s.bin",base,board,revision,names[i]);f.objects[i]=load(path,&f.bytes[i]);}return f;}
static void fixture_free(fixture *f){free(f->manifest);for(unsigned i=0;i<4;i++)free(f->objects[i]);}
static osg_content_status stage(osg_engine *e,const fixture *f,const char *manifest,size_t bytes,const char *hash){
    osg_content_status status=osg_engine_begin(e,manifest,bytes,hash,true);if(status!=OSG_CONTENT_OK)return status;bool done=false;
    while(!done){status=osg_engine_erase_step(e,&done);if(status!=OSG_CONTENT_OK)return status;}
    for(unsigned object=0;object<4;object++)for(uint32_t offset=0;offset<f->bytes[object];){size_t n=f->bytes[object]-offset;if(n>2048)n=2048;status=osg_engine_write(e,object,offset,f->objects[object]+offset,n);if(status!=OSG_CONTENT_OK)return status;offset+=(uint32_t)n;}
    return OSG_CONTENT_OK;
}
static osg_content_status install(osg_engine *e,const fixture *f){osg_content_status s=stage(e,f,f->manifest,f->manifest_bytes,f->hash);return s==OSG_CONTENT_OK?osg_engine_finish(e,true):s;}
static void fill(void *c,int x,int y,int w,int h,uint16_t rgb){(void)c;(void)x;(void)y;(void)w;(void)h;(void)rgb;}
static void text(void *c,int x,int y,int w,int h,const char *t,uint16_t rgb){(void)t;fill(c,x,y,w,h,rgb);}
static bool pixels(void *c,int x,int y,int w,int h,const uint16_t *p){osg_engine *e=c;assert(p&&x>=0&&y>=24&&w>0&&w<=64&&h>0&&h<=8&&x+w<=e->profile->width&&y+h<=e->profile->height-8);pixel_writes++;return true;}
static void display(osg_engine *e,osg_runtime *runtime,uint64_t now){assert(osg_engine_update_from_runtime(e,runtime,now));while(e->scene.pending){unsigned before=pixel_writes;osg_engine_render_slice(e,3,pixels,e);assert(pixel_writes-before<=3&&!e->failed);}if(e->scene.awaiting_present)osg_engine_presented(e,true);assert(e->ready);}
static void runtime_init(osg_engine *e,osg_image *image,osg_runtime *runtime){assert(osg_engine_make_panel(e,image)&&osg_panel_valid(&image->panel));assert(osg_init(runtime,&image->panel,(osg_display){NULL,e->profile->width,e->profile->height,fill,text},NULL,NULL));osg_connected(runtime,true);
    for(size_t i=0;i<image->panel.widget_count;i++){osg_widget *w=&image->widgets[i];if(w->kind!=OSG_BUTTON){runtime->samples[i]=(osg_sample){.available=true,.kind=w->kind==OSG_INDICATOR?OSG_BOOL:w->kind==OSG_TEXT?OSG_STRING:OSG_NUMBER,.boolean=true,.number=42};}}
}
static char *replace(const fixture *f,const char *group,const char *name,cJSON *value){cJSON *root=osg_json_parse(f->manifest,f->manifest_bytes);assert(root);cJSON *object=group?cJSON_GetObjectItemCaseSensitive(root,group):root;assert(object&&cJSON_ReplaceItemInObjectCaseSensitive(object,name,value));char *bytes=cJSON_PrintUnformatted(root);assert(bytes);cJSON_Delete(root);return bytes;}
static void reject(osg_engine *e,char *manifest){char hash[72];digest(manifest,strlen(manifest),hash);assert(osg_engine_begin(e,manifest,strlen(manifest),hash,true)==OSG_CONTENT_INVALID);cJSON_free(manifest);}
static char *serialize(cJSON *root){char *bytes=cJSON_PrintUnformatted(root);assert(bytes);cJSON_Delete(root);return bytes;}
static char *object_replace(const fixture *f,unsigned index,const char *name,cJSON *value){cJSON *root=osg_json_parse(f->manifest,f->manifest_bytes);assert(root);cJSON *object=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(root,"objects"),(int)index);assert(object&&cJSON_ReplaceItemInObjectCaseSensitive(object,name,value));return serialize(root);}
static uint16_t read16(const uint8_t *p){return (uint16_t)(p[0]|(uint16_t)p[1]<<8);}
static uint32_t read32(const uint8_t *p){return p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24;}
static void write16(uint8_t *p,unsigned n){p[0]=(uint8_t)n;p[1]=(uint8_t)(n>>8);}
static uint8_t *section(fixture *f,unsigned object,unsigned kind,unsigned *count){uint8_t *bytes=f->objects[object];for(unsigned i=0;i<read16(bytes+16);i++){uint8_t *entry=bytes+32+i*16;if(read16(entry)==kind){*count=read32(entry+8);return bytes+read32(entry+4);}}assert(false);return NULL;}
static void fixture_rehash(fixture *f){cJSON *root=osg_json_parse(f->manifest,f->manifest_bytes);assert(root);for(unsigned i=0;i<4;i++){char hash[72];digest(f->objects[i],f->bytes[i],hash);cJSON *object=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(root,"objects"),(int)i);assert(cJSON_ReplaceItemInObjectCaseSensitive(object,"sha256",cJSON_CreateString(hash)));if(!i)assert(cJSON_ReplaceItemInObjectCaseSensitive(root,"semanticDescriptorHash",cJSON_CreateString(hash)));}char *manifest=serialize(root);free(f->manifest);f->manifest_bytes=strlen(manifest);f->manifest=malloc(f->manifest_bytes+1);assert(f->manifest);memcpy(f->manifest,manifest,f->manifest_bytes+1);cJSON_free(manifest);digest(f->manifest,f->manifest_bytes,f->hash);}
static void permute_interactions(fixture *f){unsigned count;uint8_t *records=section(f,0,3,&count);assert(count==2);uint8_t temporary[24];memcpy(temporary,records,24);memcpy(records,records+24,24);memcpy(records+24,temporary,24);
    records=section(f,0,1,&count);assert(count==2);for(unsigned i=0;i<count;i++)write16(records+i*32+28,1-read16(records+i*32+28));
    records=section(f,1,8,&count);for(unsigned i=0;i<count;i++)write16(records+i*24,1-read16(records+i*24));
    records=section(f,1,3,&count);for(unsigned i=0;i<count;i++)if(records[i*24+3]==1)write16(records+i*24,1-read16(records+i*24));fixture_rehash(f);
}
static void adversarial_manifest(osg_engine *e,const fixture *a){
    assert(osg_engine_begin(e,NULL,1,a->hash,true)==OSG_CONTENT_INVALID);
    assert(osg_engine_begin(e,a->manifest,0,a->hash,true)==OSG_CONTENT_INVALID);
    assert(osg_engine_begin(e,a->manifest,OSG_ENGINE_MANIFEST_BYTES+1,a->hash,true)==OSG_CONTENT_INVALID);
    assert(osg_engine_begin(e,"{}\0",3,a->hash,true)==OSG_CONTENT_INVALID);
    assert(osg_engine_begin(e,"{",1,a->hash,true)==OSG_CONTENT_INVALID);
    for(unsigned fail=1;fail<=3;fail++){sha_failure=fail;assert(osg_engine_begin(e,a->manifest,a->manifest_bytes,a->hash,true)==OSG_CONTENT_INVALID);}sha_failure=0;
    char *space=cJSON_malloc(a->manifest_bytes+2);assert(space);space[0]=' ';memcpy(space+1,a->manifest,a->manifest_bytes);space[a->manifest_bytes+1]=0;reject(e,space);
    const char *hashes[]={"sourcePublicationHash","semanticDescriptorHash","semanticFingerprint"};for(unsigned i=0;i<3;i++)reject(e,replace(a,NULL,hashes[i],cJSON_CreateString("sha256:bad")));
    reject(e,replace(a,NULL,"compilerId",cJSON_CreateString("bad id")));reject(e,replace(a,NULL,"compilerVersion",cJSON_CreateString("")));reject(e,replace(a,NULL,"compilerVersion",cJSON_CreateString("\001")));reject(e,replace(a,NULL,"compilerVersion",cJSON_CreateString("é")));
    reject(e,replace(a,NULL,"targetProfileId",cJSON_CreateString("unknown")));reject(e,replace(a,NULL,"formatVersion",cJSON_CreateNumber(2)));
    const double numbers[]={-1,4294967296.0,0.5};for(unsigned i=0;i<3;i++)reject(e,replace(a,"counts","pages",cJSON_CreateNumber(numbers[i])));
    cJSON *root=osg_json_parse(a->manifest,a->manifest_bytes);cJSON_DeleteItemFromObjectCaseSensitive(root,"compilerId");reject(e,serialize(root));
    root=osg_json_parse(a->manifest,a->manifest_bytes);cJSON_AddBoolToObject(root,"unknown",false);reject(e,serialize(root));
    const char *resource_caps[]={"internalRamBytes","psramBytes","maxDirtyPixels","maxBlendPixelLayers","maxLayersPerTile","maxUpdateInstructions","maxPageDrawOps"};for(unsigned i=0;i<7;i++)reject(e,replace(a,"resources",resource_caps[i],cJSON_CreateNumber(99999999)));
    reject(e,replace(a,"resources","tileBytes",cJSON_CreateNumber(0)));reject(e,replace(a,"resources","transferBytes",cJSON_CreateNumber(0)));reject(e,replace(a,"resources","internalRamBytes",cJSON_CreateNumber(1)));reject(e,replace(a,"resources","cacheBytes",cJSON_CreateNumber(99999999)));reject(e,replace(a,"resources","objectBytes",cJSON_CreateNumber(0)));
    cJSON *array=cJSON_CreateArray();cJSON_AddItemToArray(array,cJSON_CreateString("scene.unsupported"));reject(e,replace(a,NULL,"requires",array));
    array=cJSON_CreateArray();cJSON_AddItemToArray(array,cJSON_CreateNumber(1));reject(e,replace(a,NULL,"requires",array));
    array=cJSON_CreateArray();for(unsigned i=0;i<33;i++)cJSON_AddItemToArray(array,cJSON_CreateString("scene.rect"));reject(e,replace(a,NULL,"requires",array));
    array=cJSON_CreateArray();cJSON_AddItemToArray(array,cJSON_CreateString("scene.rect"));cJSON_AddItemToArray(array,cJSON_CreateString("scene.rect"));reject(e,replace(a,NULL,"requires",array));
    reject(e,replace(a,"viewport","content",cJSON_CreateBool(false)));reject(e,replace(a,"viewport","content",cJSON_CreateArray()));
    array=cJSON_CreateArray();for(unsigned i=0;i<4;i++)cJSON_AddItemToArray(array,cJSON_CreateNumber(0));reject(e,replace(a,"viewport","content",array));
    reject(e,replace(a,NULL,"objects",cJSON_CreateArray()));array=cJSON_CreateArray();for(unsigned i=0;i<5;i++)cJSON_AddItemToArray(array,cJSON_CreateObject());reject(e,replace(a,NULL,"objects",array));
    reject(e,object_replace(a,0,"kind",cJSON_CreateBool(false)));reject(e,object_replace(a,0,"kind",cJSON_CreateString("unknown")));reject(e,object_replace(a,1,"kind",cJSON_CreateString("semantics")));
    reject(e,object_replace(a,0,"version",cJSON_CreateBool(false)));reject(e,object_replace(a,0,"version",cJSON_CreateNumber(2)));reject(e,object_replace(a,0,"bytes",cJSON_CreateBool(false)));reject(e,object_replace(a,0,"bytes",cJSON_CreateNumber(0)));reject(e,object_replace(a,0,"bytes",cJSON_CreateNumber(99999999)));reject(e,object_replace(a,0,"sha256",cJSON_CreateString("bad")));reject(e,object_replace(a,0,"sha256",cJSON_CreateString(a->hash)));
    /* A forged flash header must bind every manifest field before content admission. */
    const osg_content_candidate original=*osg_engine_metadata(e);osg_content_candidate altered;
    for(unsigned field=0;field<14;field++){altered=original;if(field==0)altered.manifest_bytes--;else if(field==1)altered.renderer_abi++;else if(field<6){uint8_t *hashes[]={altered.package_sha256,altered.source_sha256,altered.profile_sha256,altered.semantic_sha256};hashes[field-2][0]^=1;}else {unsigned object=(field-6)/2;if(field%2)altered.objects[object].sha256[0]^=1;else altered.objects[object].bytes++;}assert(!e->store.io.validate_manifest(e,&e->active_view,&altered));}
}
int main(int argc,char **argv){
    assert(argc==3);unsigned selected=(unsigned)strtoul(argv[2],NULL,10);assert(selected<2);sha=EVP_MD_CTX_new();assert(sha);osg_json_init();static osg_engine engine,reboot;static osg_image image;static osg_runtime runtime;
    printf("Engine working set %zu bytes\n",sizeof(engine));
    assert(osg_engine_initialize(NULL,"sunton-3248s035r",480,320)==OSG_CONTENT_ARGUMENT);
    assert(osg_engine_initialize(&engine,"unknown",480,320)==OSG_CONTENT_ARGUMENT);assert(osg_engine_initialize(&engine,"sunton-3248s035r",800,480)==OSG_CONTENT_ARGUMENT);
    assert(osg_engine_initialize(&engine,"sunton-3248s035r",480,320)==OSG_CONTENT_NOT_FOUND&&!engine.available);
    assert(!*osg_engine_profile_id(NULL)&&!*osg_engine_profile_hash(NULL)&&!*osg_engine_current_package(NULL)&&!osg_engine_metadata(NULL));
    assert(osg_engine_begin(NULL,"",0,"",false)==OSG_CONTENT_STATE&&osg_engine_finish(NULL,true)==OSG_CONTENT_STATE&&osg_engine_abort(NULL,true)==OSG_CONTENT_ARGUMENT);
    assert(!osg_engine_make_panel(NULL,&image)&&!osg_engine_update_from_runtime(NULL,&runtime,0)&&!osg_engine_copy_hits(NULL,NULL));osg_engine_presented(NULL,false);
    for(unsigned board=selected;board<=selected;board++){
        const char *name=board?"waveshare":"sunton",*profile=board?"waveshare-lcd7-s3":"sunton-3248s035r";unsigned width=board?800:480,height=board?480:320;
        fixture a=fixture_load(argv[1],name,"A"),b=fixture_load(argv[1],name,"B");flash_size=board?0x600000:0x200000;memset(flash_bytes,255,sizeof(flash_bytes));
        assert(osg_engine_initialize_io(&engine,profile,(uint16_t)width,(uint16_t)height,io())==OSG_CONTENT_NOT_FOUND&&engine.initialized&&engine.available&&!engine.active);
        bool unused=false;assert(osg_engine_erase_step(NULL,&unused)==OSG_CONTENT_STATE&&osg_engine_erase_step(&engine,NULL)==OSG_CONTENT_STATE&&osg_engine_erase_step(&engine,&unused)==OSG_CONTENT_STATE);
        assert(osg_engine_write(NULL,0,0,NULL,0)==OSG_CONTENT_STATE&&osg_engine_write(&engine,0,0,NULL,0)==OSG_CONTENT_STATE&&osg_engine_finish(&engine,true)==OSG_CONTENT_STATE);
        assert(!osg_engine_make_panel(&engine,&image)&&!osg_engine_metadata(&engine)&&!osg_engine_update_from_runtime(&engine,&runtime,0)&&!osg_engine_render_slice(NULL,1,pixels,&engine)&&!osg_engine_render_slice(&engine,1,pixels,&engine));
        osg_engine_hit_cache empty;assert(!osg_engine_copy_hits(&engine,&empty)&&!osg_engine_copy_hits(&engine,NULL));assert(osg_engine_abort(&engine,true)==OSG_CONTENT_OK);
        engine.initialized=false;assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,a.hash,true)==OSG_CONTENT_STATE);engine.initialized=true;engine.available=false;assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,a.hash,true)==OSG_CONTENT_STATE);engine.available=true;
        assert(*osg_engine_profile_id(&engine)&&*osg_engine_profile_hash(&engine)&&!*osg_engine_current_package(&engine));
        assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,a.hash,false)==OSG_CONTENT_BUSY&&osg_engine_abort(&engine,false)==OSG_CONTENT_BUSY);
        assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,"bad",true)==OSG_CONTENT_INVALID);
        assert(install(&engine,&a)==OSG_CONTENT_OK&&engine.active&&!strcmp(osg_engine_current_package(&engine),a.hash));runtime_init(&engine,&image,&runtime);display(&engine,&runtime,0);
        assert(!osg_engine_update_from_runtime(&engine,NULL,0));const osg_panel *saved=runtime.panel;runtime.panel=NULL;assert(!osg_engine_update_from_runtime(&engine,&runtime,0));runtime.panel=saved;
        osg_panel foreign=*saved;foreign.publication_hash="sha256:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";runtime.panel=&foreign;assert(!osg_engine_update_from_runtime(&engine,&runtime,0));runtime.panel=saved;
        for(unsigned status=0;status<=OSG_FEEDBACK_DISCONNECTED;status++){for(unsigned i=0;i<runtime.panel->widget_count;i++){if(image.widgets[i].kind!=OSG_BUTTON){runtime.samples[i].available=status!=0;runtime.samples[i].source_status=status;runtime.samples[i].received_ms=0;runtime.samples[i].source_age_ms=status==OSG_FEEDBACK_STALE?OSG_STALE_MS:0;}}display(&engine,&runtime,10+status);}
        runtime.armed=true;runtime.active=0;display(&engine,&runtime,15);runtime.armed=false;runtime.active=-1;
        osg_engine_hit_cache hits;assert(osg_engine_copy_hits(&engine,&hits)&&hits.ready&&hits.count>0);for(unsigned i=0;i<hits.count;i++)if(hits.hits[i].widget!=OSG_ENGINE_NONE){int x,y,w,h;osg_widget_rect(&runtime,hits.hits[i].widget,&x,&y,&w,&h);int cx=hits.hits[i].x+hits.hits[i].width/2,cy=hits.hits[i].y+hits.hits[i].height/2;assert(cx>=x&&cy>=y&&cx<x+w&&cy<y+h);}
        assert(osg_engine_initialize_io(&reboot,profile,(uint16_t)width,(uint16_t)height,io())==OSG_CONTENT_OK&&!strcmp(osg_engine_current_package(&reboot),a.hash)&&!reboot.ready);osg_content_release(&reboot.active_view);
        adversarial_manifest(&engine,&a);
        assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,a.hash,true)==OSG_CONTENT_OK);assert(osg_engine_begin(&engine,a.manifest,a.manifest_bytes,a.hash,true)==OSG_CONTENT_STATE&&osg_engine_write(&engine,4,0,NULL,0)==OSG_CONTENT_STATE);assert(osg_engine_abort(&engine,true)==OSG_CONTENT_OK);
        runtime_init(&engine,&image,&runtime);for(unsigned phase=0;phase<3;phase++){for(unsigned i=0;i<runtime.panel->widget_count;i++)if(image.widgets[i].kind!=OSG_BUTTON){runtime.samples[i].available=true;runtime.samples[i].source_status=OSG_FEEDBACK_AUTOMATIC;runtime.samples[i].received_ms=phase==0?1000:0;runtime.samples[i].source_age_ms=phase==1?OSG_STALE_MS:0;}display(&engine,&runtime,30+phase);}
        const char *keys[]={"compilerId","compilerVersion","counts","formatVersion","objects","rendererAbi","requires","resources","semanticDescriptorHash","semanticFingerprint","sourcePublicationHash","sourceUiId","targetProfileHash","targetProfileId","viewport"};for(unsigned i=0;i<sizeof(keys)/sizeof(keys[0]);i++)reject(&engine,replace(&a,NULL,keys[i],cJSON_CreateBool(false)));
        const char *counts[]={"bindings","glyphs","hitRegions","nodes","pages","timelines","variables"};for(unsigned i=0;i<7;i++)reject(&engine,replace(&a,"counts",counts[i],cJSON_CreateNumber(999999)));
        const char *resources[]={"cacheBytes","internalRamBytes","maxBlendPixelLayers","maxDirtyPixels","maxLayersPerTile","maxPageDrawOps","maxUpdateInstructions","objectBytes","psramBytes","sceneRamBytes","snapshotBytes","tileBytes","transferBytes"};for(unsigned i=0;i<13;i++)reject(&engine,replace(&a,"resources",resources[i],cJSON_CreateBool(false)));
        reject(&engine,replace(&a,"viewport","width",cJSON_CreateNumber(17)));reject(&engine,replace(&a,"viewport","height",cJSON_CreateNumber(17)));reject(&engine,replace(&a,NULL,"sourceUiId",cJSON_CreateString("bad id")));
        reject(&engine,replace(&a,NULL,"rendererAbi",cJSON_CreateNumber(2)));reject(&engine,replace(&a,NULL,"targetProfileHash",cJSON_CreateString(a.hash)));
        char *bad=replace(&b,"resources","maxDirtyPixels",cJSON_CreateNumber(0)),hash[72];digest(bad,strlen(bad),hash);assert(stage(&engine,&b,bad,strlen(bad),hash)==OSG_CONTENT_OK);cJSON_free(bad);
        assert(osg_engine_finish(&engine,false)==OSG_CONTENT_BUSY);assert(osg_engine_finish(&engine,true)==OSG_CONTENT_INVALID&&engine.active&&!strcmp(engine.package_hash,a.hash));runtime_init(&engine,&image,&runtime);display(&engine,&runtime,20);
        assert(stage(&engine,&b,b.manifest,b.manifest_bytes,b.hash)==OSG_CONTENT_OK);fail_commit=true;assert(osg_engine_finish(&engine,true)==OSG_CONTENT_IO);fail_commit=false;assert(engine.active&&!strcmp(engine.package_hash,a.hash));
        assert(stage(&engine,&b,b.manifest,b.manifest_bytes,b.hash)==OSG_CONTENT_OK);uint32_t damaged=engine.store.slot_offsets[engine.store.stage_slot]+4096+engine.store.slots[engine.store.stage_slot].offsets[1];flash_bytes[damaged]^=1;assert(osg_engine_finish(&engine,true)==OSG_CONTENT_HASH&&engine.active&&!strcmp(engine.package_hash,a.hash));
        /* Internal record indices are not logical ownership: pressed visuals must survive a permutation. */
        assert(install(&engine,&b)==OSG_CONTENT_OK);runtime_init(&engine,&image,&runtime);runtime.armed=true;runtime.active=0;display(&engine,&runtime,80);osg_scene_properties expected[256];memcpy(expected,engine.scene.properties[engine.scene.displayed],sizeof(expected));
        permute_interactions(&b);assert(install(&engine,&b)==OSG_CONTENT_OK);runtime_init(&engine,&image,&runtime);runtime.armed=true;runtime.active=0;display(&engine,&runtime,80);assert(!memcmp(expected,engine.scene.properties[engine.scene.displayed],engine.scene.nodes*sizeof(expected[0])));
        assert(install(&engine,&b)==OSG_CONTENT_OK&&!strcmp(engine.package_hash,b.hash));runtime_init(&engine,&image,&runtime);display(&engine,&runtime,100);osg_connected(&runtime,false);display(&engine,&runtime,200);assert(osg_engine_copy_hits(&engine,&hits)&&!hits.ready&&!hits.count);
        osg_connected(&runtime,true);display(&engine,&runtime,300);read_failure=true;hits.count=31;
        assert(!osg_engine_copy_hits(&engine,&hits)&&!hits.count&&engine.failed&&engine.scene.failed&&!engine.ready&&!engine.scene.ready);read_failure=false;
        assert(!osg_engine_copy_hits(&engine,&hits)&&!hits.count);
        assert(osg_engine_initialize_io(&reboot,profile,(uint16_t)width,(uint16_t)height,io())==OSG_CONTENT_OK&&!strcmp(reboot.package_hash,b.hash));osg_content_release(&reboot.active_view);osg_content_release(&engine.active_view);
        fixture_free(&a);fixture_free(&b);
    }
    assert(!osg_json_allocated());EVP_MD_CTX_free(sha);puts("Both-board A/B streamed engine install, canonical manifest/profile/resource rejection, panel shim, variable redraw, warm scan and rollback tests passed");return 0;
}
