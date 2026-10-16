#include "engine.h"
#include "profiles.generated.h"
#include <math.h>
#include <string.h>
#ifdef ESP_PLATFORM
#include "content_store_esp.h"
static osg_content_esp platform_store;
#endif
#define REQUIRE(x) do { if(!(x)) return false; } while(0)
#define COUNT(a) (sizeof(a)/sizeof((a)[0]))
static const char *const object_names[]={"semantics","scene","images","fonts"};
static const char *const features[]={"input.momentary","input.navigation","scene.image565","scene.image565-mask4","scene.line","scene.local-state","scene.mask4","scene.polyline","scene.rect","scene.text","scene.timeline","scene.timeline-repeat"};
static uint16_t u16(const uint8_t *p){return (uint16_t)(p[0]|(uint16_t)p[1]<<8);}
static uint32_t u32(const uint8_t *p){return p[0]|(uint32_t)p[1]<<8|(uint32_t)p[2]<<16|(uint32_t)p[3]<<24;}
static int16_t s16(const uint8_t *p){uint16_t n=u16(p);return n<=INT16_MAX?(int16_t)n:(int16_t)(-1-(int)(UINT16_MAX-n));}
static int32_t s32(const uint8_t *p){uint32_t n=u32(p);return n<=INT32_MAX?(int32_t)n:-1-(int32_t)(UINT32_MAX-n);}
static const cJSON *field(const cJSON *value,const char *name){return cJSON_GetObjectItemCaseSensitive(value,name);}
static bool exact(const cJSON *v,const char *const *keys,size_t n){
    REQUIRE(cJSON_IsObject(v));size_t count=0;
    for(const cJSON *p=v->child;p;p=p->next){bool found=false;for(size_t i=0;i<n;i++)if(!strcmp(p->string,keys[i]))found=true;REQUIRE(found);count++;}
    return count==n;
}
static bool sorted(const cJSON *v){
    const char *previous=NULL;
    for(const cJSON *p=v->child;p;p=p->next){if(cJSON_IsObject(v)){REQUIRE(!previous||strcmp(previous,p->string)<0);previous=p->string;}REQUIRE(sorted(p));}
    return true;
}
static bool uint_value(const cJSON *v,uint32_t *out){REQUIRE(cJSON_IsNumber(v)&&isfinite(v->valuedouble)&&v->valuedouble>=0&&v->valuedouble<=UINT32_MAX&&floor(v->valuedouble)==v->valuedouble);*out=(uint32_t)v->valuedouble;return true;}
static bool quantity(const cJSON *v,const char *name,uint32_t *out){return uint_value(field(v,name),out);}
static bool cap(const cJSON *v,const char *name,uint32_t maximum){uint32_t value;return quantity(v,name,&value)&&value<=maximum;}
static bool hash_decode(const char *text,uint8_t out[32]){REQUIRE(osg_hash_id(text));for(unsigned i=0;i<32;i++){unsigned a=(unsigned char)text[7+i*2],b=(unsigned char)text[8+i*2];a=a<='9'?a-'0':a-'a'+10;b=b<='9'?b-'0':b-'a'+10;out[i]=(uint8_t)(a*16+b);}return true;}
static void hash_encode(const uint8_t bytes[32],char out[72]){static const char digits[]="0123456789abcdef";memcpy(out,"sha256:",7);for(unsigned i=0;i<32;i++){out[7+i*2]=digits[bytes[i]>>4];out[8+i*2]=digits[bytes[i]&15];}out[71]=0;}
static bool hash_memory(osg_engine *e,const void *bytes,size_t n,uint8_t out[32]){
    osg_content_io *io=&e->store.io;REQUIRE(io->hash_begin(io->hash_context));const uint8_t *p=bytes;
    while(n){size_t step=n>2048?2048:n;REQUIRE(io->hash_update(io->hash_context,p,step));p+=step;n-=step;}
    return io->hash_finish(io->hash_context,out);
}
static bool ascii_text(const char *s,size_t maximum,bool empty){if(!s||(!empty&&!*s)||strlen(s)>maximum)return false;for(;*s;s++)if((unsigned char)*s<32||(unsigned char)*s>126)return false;return true;}
static uint32_t section_count(const osg_scene *s,unsigned object,unsigned section){for(unsigned i=0;i<s->object[object].section_count;i++)if(s->object[object].sections[i].kind==section)return s->object[object].sections[i].count;return 0;}
static bool has_feature(const cJSON *list,const char *name){for(const cJSON *p=list->child;p;p=p->next)if(cJSON_IsString(p)&&!strcmp(p->valuestring,name))return true;return false;}
static bool candidate_equal(const osg_content_candidate *a,const osg_content_candidate *b){
    if(a->manifest_bytes!=b->manifest_bytes||a->renderer_abi!=b->renderer_abi||memcmp(a->package_sha256,b->package_sha256,32)||memcmp(a->source_sha256,b->source_sha256,32)||memcmp(a->profile_sha256,b->profile_sha256,32)||memcmp(a->semantic_sha256,b->semantic_sha256,32))return false;
    for(unsigned i=0;i<4;i++)if(a->objects[i].bytes!=b->objects[i].bytes||memcmp(a->objects[i].sha256,b->objects[i].sha256,32))return false;
    return true;
}
static bool manifest_shape(osg_engine *e,const cJSON *root,const char *expected_hash,osg_content_candidate *candidate){
    static const char *const keys[]={"compilerId","compilerVersion","counts","formatVersion","objects","rendererAbi","requires","resources","semanticDescriptorHash","semanticFingerprint","sourcePublicationHash","sourceUiId","targetProfileHash","targetProfileId","viewport"};
    static const char *const count_keys[]={"bindings","glyphs","hitRegions","nodes","pages","timelines","variables"};
    static const char *const resource_keys[]={"cacheBytes","internalRamBytes","maxBlendPixelLayers","maxDirtyPixels","maxLayersPerTile","maxPageDrawOps","maxUpdateInstructions","objectBytes","psramBytes","sceneRamBytes","snapshotBytes","tileBytes","transferBytes"};
    static const char *const viewport_keys[]={"content","height","width"},*const object_keys[]={"bytes","kind","sha256","version"};
    REQUIRE(exact(root,keys,COUNT(keys))&&sorted(root));uint32_t value=0;REQUIRE(quantity(root,"formatVersion",&value)&&value==1&&quantity(root,"rendererAbi",&value)&&value==1);
    REQUIRE(osg_stable_id(osg_string(root,"sourceUiId"))&&osg_stable_id(osg_string(root,"compilerId"))&&ascii_text(osg_string(root,"compilerVersion"),64,false));
    REQUIRE(!strcmp(osg_string(root,"targetProfileId")?osg_string(root,"targetProfileId"):"",e->profile_id)&&!strcmp(osg_string(root,"targetProfileHash")?osg_string(root,"targetProfileHash"):"",e->profile_hash));
    memset(candidate,0,sizeof(*candidate));candidate->renderer_abi=1;candidate->manifest_bytes=(uint32_t)e->manifest_bytes;
    uint8_t fingerprint[32];REQUIRE(hash_decode(expected_hash,candidate->package_sha256)&&hash_decode(osg_string(root,"sourcePublicationHash"),candidate->source_sha256)&&hash_decode(osg_string(root,"targetProfileHash"),candidate->profile_sha256)&&hash_decode(osg_string(root,"semanticDescriptorHash"),candidate->semantic_sha256)&&hash_decode(osg_string(root,"semanticFingerprint"),fingerprint));
    const cJSON *viewport=field(root,"viewport"),*content=field(viewport,"content");REQUIRE(exact(viewport,viewport_keys,COUNT(viewport_keys))&&quantity(viewport,"width",&value)&&value==e->profile->width&&quantity(viewport,"height",&value)&&value==e->profile->height&&cJSON_IsArray(content)&&cJSON_GetArraySize(content)==4);
    const uint32_t rectangle[]={(uint32_t)e->profile->x,(uint32_t)e->profile->y,e->profile->content_width,e->profile->content_height};for(unsigned i=0;i<4;i++)REQUIRE(uint_value(cJSON_GetArrayItem(content,(int)i),&value)&&value==rectangle[i]);
    const cJSON *requires=field(root,"requires");REQUIRE(cJSON_IsArray(requires)&&cJSON_GetArraySize(requires)<=32);const char *previous=NULL;
    for(const cJSON *p=requires->child;p;p=p->next){REQUIRE(cJSON_IsString(p)&&(!previous||strcmp(previous,p->valuestring)<0));bool supported=false;for(unsigned i=0;i<COUNT(features);i++)if(!strcmp(features[i],p->valuestring))supported=true;REQUIRE(supported);previous=p->valuestring;}
    const cJSON *counts=field(root,"counts"),*resources=field(root,"resources"),*objects=field(root,"objects");REQUIRE(exact(counts,count_keys,COUNT(count_keys))&&exact(resources,resource_keys,COUNT(resource_keys))&&cJSON_IsArray(objects)&&cJSON_GetArraySize(objects)>=2&&cJSON_GetArraySize(objects)<=4);
    const osg_render_package_limits *limits=e->profile->limits;
    REQUIRE(cap(counts,"pages",limits->pages)&&cap(counts,"nodes",limits->nodes)&&cap(counts,"variables",limits->variables)&&cap(counts,"bindings",limits->bindings)&&cap(counts,"hitRegions",limits->hit_regions)&&cap(counts,"glyphs",limits->glyphs)&&cap(counts,"timelines",limits->timelines));
    uint64_t object_bytes=0;int last=-1;
    for(const cJSON *object=objects->child;object;object=object->next){REQUIRE(exact(object,object_keys,COUNT(object_keys)));int kind=-1;const char *name=osg_string(object,"kind");for(unsigned i=0;name&&i<4;i++)if(!strcmp(name,object_names[i]))kind=(int)i;
        REQUIRE(kind>last&&quantity(object,"version",&value)&&value==1&&quantity(object,"bytes",&value)&&value>=32&&value<=limits->object_bytes[kind]);last=kind;candidate->objects[kind].bytes=value;object_bytes+=value;REQUIRE(hash_decode(osg_string(object,"sha256"),candidate->objects[kind].sha256));}
    REQUIRE(candidate->objects[0].bytes&&candidate->objects[1].bytes&&!memcmp(candidate->semantic_sha256,candidate->objects[0].sha256,32)&&quantity(resources,"objectBytes",&value)&&value==object_bytes&&object_bytes+e->manifest_bytes<=e->store.io.package_limit);
    const bool wave=e->profile->width==800;
    REQUIRE(cap(resources,"internalRamBytes",wave?32768:24576)&&cap(resources,"psramBytes",wave?1048576:0)&&cap(resources,"maxDirtyPixels",e->profile->dirty_pixels)&&cap(resources,"maxBlendPixelLayers",e->profile->blend_pixels)&&cap(resources,"maxLayersPerTile",e->profile->layers)&&cap(resources,"maxUpdateInstructions",limits->expression_instructions)&&cap(resources,"maxPageDrawOps",limits->nodes));
    uint32_t internal,psram,tile,transfer;REQUIRE(quantity(resources,"internalRamBytes",&internal)&&quantity(resources,"psramBytes",&psram)&&quantity(resources,"tileBytes",&tile)&&tile==1024&&quantity(resources,"transferBytes",&transfer)&&transfer==2048);
    uint64_t subdivisions=tile+transfer;const char *const ram_parts[]={"sceneRamBytes","snapshotBytes","cacheBytes"};for(unsigned i=0;i<COUNT(ram_parts);i++){REQUIRE(quantity(resources,ram_parts[i],&value));subdivisions+=value;}REQUIRE(subdivisions<=(uint64_t)internal+psram&&internal>=sizeof(*e)+OSG_ENGINE_SCENE_WORK_BYTES+OSG_ENGINE_AUXILIARY_BYTES);
    return true;
}
static bool parse_manifest(osg_engine *e,const char *bytes,size_t n,const char *hash,osg_content_candidate *candidate){
    REQUIRE(bytes&&n&&n<=e->store.io.manifest_limit&&n<=OSG_ENGINE_MANIFEST_BYTES&&!memchr(bytes,0,n));memmove(e->manifest,bytes,n);e->manifest[n]=0;e->manifest_bytes=n;
    cJSON *root=osg_json_parse(e->manifest,n);if(!root)return false;
    bool ok=manifest_shape(e,root,hash,candidate)&&cJSON_PrintPreallocated(root,e->manifest,sizeof(e->manifest),false);uint8_t digest[32];
    if(ok)ok=strlen(e->manifest)==n&&hash_memory(e,e->manifest,n,digest)&&!memcmp(digest,candidate->package_sha256,32);
    cJSON_Delete(root);return ok;
}
static bool read_manifest(osg_engine *e,const osg_content_view *view,const osg_content_candidate *candidate){
    if(candidate->manifest_bytes>OSG_ENGINE_MANIFEST_BYTES)return false;
    for(uint32_t offset=0;offset<candidate->manifest_bytes;){size_t n=candidate->manifest_bytes-offset;if(n>2048)n=2048;REQUIRE(osg_content_read(view,OSG_CONTENT_MANIFEST,offset,e->manifest+offset,n)==OSG_CONTENT_OK);offset+=(uint32_t)n;}
    char hash[72];hash_encode(candidate->package_sha256,hash);osg_content_candidate parsed;return parse_manifest(e,e->manifest,candidate->manifest_bytes,hash,&parsed)&&candidate_equal(candidate,&parsed);
}
static bool validate_manifest(void *context,const osg_content_view *view,const osg_content_candidate *candidate){return read_manifest(context,view,candidate);}
typedef struct {unsigned page;int x,y,w,h;unsigned interaction;} button_box;
static bool legacy_text(const char *value){if(!ascii_text(value,120,true))return false;for(;*value;value++){unsigned char c=(unsigned char)*value;if(!((c>='a'&&c<='z')||(c>='A'&&c<='Z')||(c>='0'&&c<='9')||strchr(" .,:;-_+=|/%!()?",c)))return false;}return true;}
static const char *put_string(osg_image *out,size_t *used,const char *value){size_t n=strlen(value)+1;if(n>OSG_IMAGE_TEXT_BYTES-*used)return NULL;const char *result="";if(out){memcpy(out->strings+*used,value,n);result=out->strings+*used;}*used+=n;return result;}
static bool record_string(osg_engine *e,unsigned object,unsigned section,const uint8_t *record,unsigned offset_at,unsigned bytes_at,char out[121]){return osg_scene_string(&e->scene,object,section,u32(record+offset_at),u16(record+bytes_at),out,121);}
static bool point_in(button_box b,int x,int y){return x>=b.x&&y>=b.y&&x<b.x+b.w&&y<b.y+b.h;}
static bool spare_pixel(osg_engine *e,const button_box *buttons,unsigned controls,unsigned *page,int *x,int *y){
    for(unsigned p=0;p<e->scene.pages;p++)for(unsigned yi=0;yi<=controls;yi++)for(unsigned xi=0;xi<=controls;xi++){
        if((yi&&buttons[yi-1].page!=p)||(xi&&buttons[xi-1].page!=p))continue;
        int px=xi?buttons[xi-1].x+buttons[xi-1].w:e->profile->x,py=yi?buttons[yi-1].y+buttons[yi-1].h:e->profile->y;
        if(px>=e->profile->x+e->profile->content_width||py>=e->profile->y+e->profile->content_height)continue;
        bool covered=false;for(unsigned i=0;i<controls;i++)if(buttons[i].page==p&&point_in(buttons[i],px,py))covered=true;
        if(!covered){*page=p;*x=px;*y=py;return true;}
    }
    return false;
}
static bool shim(osg_engine *e,osg_image *out){
    unsigned controls=section_count(&e->scene,0,1),outputs=section_count(&e->scene,0,2);REQUIRE(controls+outputs>0&&controls+outputs<=32&&e->scene.pages<=4);
    size_t used=0;char id[121],channel[121],unit[121],all_ids[32][65];uint8_t record[32],hit[24];button_box buttons[32];
    if(out){memset(out,0,sizeof(*out));for(unsigned i=0;i<32;i++){e->control_widgets[i]=e->interaction_widgets[i]=OSG_ENGINE_NONE;}for(unsigned i=0;i<64;i++)e->output_widgets[i]=OSG_ENGINE_NONE;}
    const char *ui_id=put_string(out,&used,e->ui_id),*publication=put_string(out,&used,e->source_hash);REQUIRE(ui_id&&publication);
    for(unsigned page=0;page<e->scene.pages;page++){
        REQUIRE(osg_scene_record(&e->scene,1,1,page,record,16)&&record_string(e,1,10,record,0,4,id)&&osg_stable_id(id));const char *page_id=put_string(out,&used,id);REQUIRE(page_id);
        if(out)out->pages[page]=(osg_page){page_id,"Panel",e->profile->width,(uint16_t)(e->profile->height-32)};
    }
    for(unsigned i=0;i<controls;i++){
        REQUIRE(osg_scene_record(&e->scene,0,1,i,record,32)&&record[6]==1&&record_string(e,0,4,record,0,4,id)&&osg_stable_id(id));memcpy(all_ids[i],id,strlen(id)+1);
        unsigned interaction=u16(record+28),found=0;button_box box={0};
        for(unsigned j=0;j<e->scene.hits;j++){REQUIRE(osg_scene_record(&e->scene,1,8,j,hit,24));if(u16(hit)==interaction){found++;box=(button_box){u16(hit+2),s16(hit+4),s16(hit+6),u16(hit+8),u16(hit+10),interaction};}}
        REQUIRE(found==1&&interaction<32);for(unsigned j=0;j<i;j++)REQUIRE(box.page!=buttons[j].page||box.x>=buttons[j].x+buttons[j].w||buttons[j].x>=box.x+box.w||box.y>=buttons[j].y+buttons[j].h||buttons[j].y>=box.y+box.h);
        buttons[i]=box;const char *control_id=put_string(out,&used,id);REQUIRE(control_id);
        if(out){osg_widget *w=&out->widgets[i];*w=(osg_widget){.id=control_id,.reference=control_id,.output_id="",.label="Input",.prefix="",.suffix="",.unit="",.kind=OSG_BUTTON,.page=(uint8_t)box.page,
                .bounds={(double)(box.x-e->profile->x)/e->profile->content_width,(double)(box.y-e->profile->y)/e->profile->content_height,(double)box.w/e->profile->content_width,(double)box.h/e->profile->content_height}};
            e->control_widgets[i]=(uint16_t)i;e->interaction_widgets[interaction]=(uint16_t)i;}
    }
    unsigned output_page=0;int output_x=e->profile->x,output_y=e->profile->y;REQUIRE(!outputs||spare_pixel(e,buttons,controls,&output_page,&output_x,&output_y));
    for(unsigned i=0;i<outputs;i++){
        REQUIRE(osg_scene_record(&e->scene,0,2,i,record,32)&&record_string(e,0,4,record,0,4,id)&&osg_stable_id(id)&&record_string(e,0,4,record,8,12,channel)&&osg_stable_id(channel)&&record_string(e,0,4,record,24,28,unit)&&legacy_text(unit));
        for(unsigned j=0;j<controls+i;j++)REQUIRE(strcmp(all_ids[j],id));
        memcpy(all_ids[controls+i],id,strlen(id)+1);
        REQUIRE(record[6]!=2||s32(record+16)<s32(record+20));
        const char *output_id=put_string(out,&used,id),*channel_id=put_string(out,&used,channel),*unit_value=put_string(out,&used,unit);REQUIRE(output_id&&channel_id&&unit_value);
        if(out){unsigned index=controls+i;out->widgets[index]=(osg_widget){.id=output_id,.reference=channel_id,.output_id=output_id,.label="Feedback",.prefix="",.suffix="",.unit=unit_value,
            .kind=record[6]==1?OSG_INDICATOR:record[6]==2?OSG_NUMERIC:OSG_TEXT,.page=(uint8_t)output_page,
            .bounds={(double)(output_x-e->profile->x)/e->profile->content_width,(double)(output_y-e->profile->y)/e->profile->content_height,1.0/e->profile->content_width,1.0/e->profile->content_height},
            .min=(double)s32(record+16)/65536,.max=(double)s32(record+20)/65536};e->output_widgets[i]=(uint16_t)index;}
    }
    if(out){out->used=used;out->panel=(osg_panel){ui_id,publication,out->pages,out->widgets,e->scene.pages,controls+outputs};REQUIRE(osg_panel_valid(&out->panel));}
    return true;
}
static bool usage_matches(osg_engine *e,const cJSON *root){
    osg_scene_usage usage;REQUIRE(osg_scene_get_usage(&e->scene,&usage));const cJSON *counts=field(root,"counts"),*resources=field(root,"resources");uint32_t value;
    const char *const names[]={"pages","nodes","variables","bindings","hitRegions","glyphs","timelines"};const uint32_t derived[]={usage.pages,usage.nodes,usage.variables,usage.bindings,usage.hit_regions,usage.glyphs,usage.timelines};
    for(unsigned i=0;i<COUNT(names);i++)REQUIRE(quantity(counts,names[i],&value)&&value==derived[i]);
    const char *const budgets[]={"maxPageDrawOps","maxUpdateInstructions","maxDirtyPixels","maxLayersPerTile","maxBlendPixelLayers","snapshotBytes","sceneRamBytes"};
    const uint32_t facts[]={usage.max_page_draw_ops,usage.max_update_instructions,usage.max_dirty_pixels,usage.max_layers_per_tile,usage.max_blend_pixel_layers,usage.minimum_snapshot_bytes,usage.runtime_bytes};
    for(unsigned i=0;i<COUNT(budgets);i++)REQUIRE(quantity(resources,budgets[i],&value)&&value>=facts[i]);
    const cJSON *requires=field(root,"requires");uint8_t record[64];
    for(unsigned i=0;i<e->scene.nodes;i++){REQUIRE(osg_scene_record(&e->scene,1,2,i,record,64));const char *op[]={"","scene.rect","scene.image565","scene.mask4","scene.image565-mask4","scene.text","scene.line","scene.polyline"};REQUIRE(record[2]<COUNT(op)&&has_feature(requires,op[record[2]]));}
    for(unsigned i=0;i<usage.controls;i++)REQUIRE(has_feature(requires,"input.momentary"));
    for(unsigned i=0;i<section_count(&e->scene,0,3);i++){REQUIRE(osg_scene_record(&e->scene,0,3,i,record,24));if(record[6]==2)REQUIRE(has_feature(requires,"input.navigation"));}
    for(unsigned i=0;i<usage.variables;i++){REQUIRE(osg_scene_record(&e->scene,1,3,i,record,24));if(record[3])REQUIRE(has_feature(requires,"scene.local-state"));}
    for(unsigned i=0;i<usage.timelines;i++){REQUIRE(has_feature(requires,"scene.timeline")&&osg_scene_record(&e->scene,1,6,i,record,24));if(u16(record+10)&1)REQUIRE(has_feature(requires,"scene.timeline-repeat"));}
    return true;
}
static bool admit_view(osg_engine *e,const osg_content_view *view,const osg_content_candidate *candidate){
    e->validation_view=*view;osg_scene_source sources[4];for(unsigned i=0;i<4;i++){e->readers[i]=(osg_content_reader){&e->validation_view,i};sources[i]=(osg_scene_source){osg_content_object_read,&e->readers[i],candidate->objects[i].bytes};}
    REQUIRE(osg_scene_admit(&e->scene,sources,e->profile));cJSON *root=osg_json_parse(e->manifest,e->manifest_bytes);if(!root)return false;
    const char *ui_id=osg_string(root,"sourceUiId"),*source=osg_string(root,"sourcePublicationHash");
    /* Shim admission uses prospective identity only while rendering is suspended;
     * callers restore the old manifest/scene if this candidate fails. */
    char old_ui[65],old_source[72];memcpy(old_ui,e->ui_id,sizeof(old_ui));memcpy(old_source,e->source_hash,sizeof(old_source));
    bool ok=ui_id&&source&&osg_copy(e->ui_id,sizeof(e->ui_id),ui_id)&&osg_copy(e->source_hash,sizeof(e->source_hash),source)&&usage_matches(e,root)&&shim(e,NULL);
    memcpy(e->ui_id,old_ui,sizeof(old_ui));memcpy(e->source_hash,old_source,sizeof(old_source));cJSON_Delete(root);return ok;
}
static bool validate_content(void *context,const osg_content_view *view,const osg_content_candidate *candidate){return admit_view(context,view,candidate);}
static bool configure(osg_engine *e,const char *board,uint16_t width,uint16_t height){
    if(!e||!board)return false;
    memset(e,0,sizeof(*e));
    if(!strcmp(board,"sunton-3248s035r")){e->profile=&OSG_SCENE_SUNTON;e->profile_id=OSG_RENDER_SUNTON_PROFILE_ID;e->profile_hash=OSG_RENDER_SUNTON_PROFILE_HASH;}
    else if(!strcmp(board,"waveshare-lcd7-s3")){e->profile=&OSG_SCENE_WAVESHARE;e->profile_id=OSG_RENDER_WAVESHARE_PROFILE_ID;e->profile_hash=OSG_RENDER_WAVESHARE_PROFILE_HASH;}
    else return false;
    return width==e->profile->width&&height==e->profile->height;
}
static bool identity(osg_engine *e,const osg_content_candidate *candidate){
    cJSON *root=osg_json_parse(e->manifest,e->manifest_bytes);if(!root)return false;
    bool ok=osg_copy(e->ui_id,sizeof(e->ui_id),osg_string(root,"sourceUiId"))&&osg_copy(e->source_hash,sizeof(e->source_hash),osg_string(root,"sourcePublicationHash"))&&osg_copy(e->semantic_hash,sizeof(e->semantic_hash),osg_string(root,"semanticDescriptorHash"));
    cJSON_Delete(root);if(ok)hash_encode(candidate->package_sha256,e->package_hash);return ok;
}
static bool restore_active(osg_engine *e){
    if(!e->active_view.held){e->active=false;e->ready=false;memset(&e->scene,0,sizeof(e->scene));return true;}
    const osg_content_candidate *candidate=osg_content_metadata(&e->active_view);
    if(!candidate||!read_manifest(e,&e->active_view,candidate)||!admit_view(e,&e->active_view,candidate)||!identity(e,candidate)){e->active=e->ready=false;e->failed=true;return false;}
    e->active=true;e->ready=e->failed=false;return true;
}
static bool activate_current(osg_engine *e){
    osg_content_view next;REQUIRE(osg_content_acquire(&e->store,&next)==OSG_CONTENT_OK);
    const osg_content_candidate *candidate=osg_content_metadata(&next);
    if(!candidate||!read_manifest(e,&next,candidate)||!admit_view(e,&next,candidate)||!identity(e,candidate)){osg_content_release(&next);return false;}
    osg_content_release(&e->active_view);e->active_view=next;e->validation_view=next;e->active=true;e->ready=e->failed=false;return true;
}
static osg_content_status initialized(osg_engine *e,osg_content_status status){
    if(status!=OSG_CONTENT_OK){e->status=status;return status;}e->initialized=e->available=true;status=osg_content_scan(&e->store);
    if(status==OSG_CONTENT_OK&&!activate_current(e))status=OSG_CONTENT_INVALID;
    e->status=status;e->failed=status!=OSG_CONTENT_OK&&status!=OSG_CONTENT_NOT_FOUND;return status;
}
osg_content_status osg_engine_initialize_io(osg_engine *e,const char *board,uint16_t width,uint16_t height,osg_content_io io){
    if(!configure(e,board,width,height))return OSG_CONTENT_ARGUMENT;
    io.manifest_limit=width==800?8192:4096;io.package_limit=(width==800?2560:768)*1024;io.validation_context=e;io.validate_manifest=validate_manifest;io.validate_content=validate_content;
    return initialized(e,osg_content_init(&e->store,io));
}
osg_content_status osg_engine_initialize(osg_engine *e,const char *board,uint16_t width,uint16_t height){
    if(!configure(e,board,width,height))return OSG_CONTENT_ARGUMENT;
#ifdef ESP_PLATFORM
    return initialized(e,osg_content_esp_open(&platform_store,&e->store,board,e,validate_manifest,validate_content));
#else
    e->status=OSG_CONTENT_NOT_FOUND;return e->status;
#endif
}
const char *osg_engine_profile_id(const osg_engine *e){return e&&e->profile_id?e->profile_id:"";}
const char *osg_engine_profile_hash(const osg_engine *e){return e&&e->profile_hash?e->profile_hash:"";}
const char *osg_engine_current_package(const osg_engine *e){return e&&e->active?e->package_hash:"";}
const osg_content_candidate *osg_engine_metadata(const osg_engine *e){return e&&e->active?osg_content_metadata(&e->active_view):NULL;}
osg_content_status osg_engine_begin(osg_engine *e,const char *bytes,size_t n,const char *hash,bool suspended){
    if(!e||!e->initialized||!e->available||e->staging)return OSG_CONTENT_STATE;
    if(!suspended)return OSG_CONTENT_BUSY;
    if(!parse_manifest(e,bytes,n,hash,&e->pending))return e->status=OSG_CONTENT_INVALID;
    e->status=osg_content_begin(&e->store,&e->pending);e->staging=e->status==OSG_CONTENT_OK;return e->status;
}
osg_content_status osg_engine_erase_step(osg_engine *e,bool *done){
    if(!e||!done||!e->staging)return OSG_CONTENT_STATE;
    e->status=osg_content_erase_step(&e->store,done);if(e->status!=OSG_CONTENT_OK||!*done)return e->status;
    for(uint32_t offset=0;offset<e->pending.manifest_bytes;){size_t n=e->pending.manifest_bytes-offset;if(n>2048)n=2048;e->status=osg_content_write(&e->store,OSG_CONTENT_MANIFEST,offset,e->manifest+offset,n);if(e->status!=OSG_CONTENT_OK)return e->status;offset+=(uint32_t)n;}
    return e->status;
}
osg_content_status osg_engine_write(osg_engine *e,unsigned object,uint32_t offset,const void *bytes,size_t n){
    if(!e||!e->staging||object>=4)return OSG_CONTENT_STATE;
    return e->status=osg_content_write(&e->store,object,offset,bytes,n);
}
osg_content_status osg_engine_finish(osg_engine *e,bool suspended){
    if(!e||!e->staging)return OSG_CONTENT_STATE;
    if(!suspended)return OSG_CONTENT_BUSY;
    osg_content_status status=osg_content_verify(&e->store);if(status==OSG_CONTENT_OK)status=osg_content_commit(&e->store);
    if(status==OSG_CONTENT_OK){if(!activate_current(e))status=OSG_CONTENT_INVALID;}
    if(status!=OSG_CONTENT_OK){osg_content_abort(&e->store);(void)restore_active(e);}e->staging=false;e->status=status;return status;
}
osg_content_status osg_engine_abort(osg_engine *e,bool suspended){
    if(!e)return OSG_CONTENT_ARGUMENT;
    if(!suspended)return OSG_CONTENT_BUSY;
    osg_content_abort(&e->store);e->staging=false;return e->status=restore_active(e)?OSG_CONTENT_OK:OSG_CONTENT_INVALID;
}
bool osg_engine_make_panel(osg_engine *e,osg_image *out){return e&&e->active&&out&&shim(e,out);}
bool osg_engine_update_from_runtime(osg_engine *e,const osg_runtime *runtime,uint64_t now){
    if(!e||!e->active||!runtime||!runtime->panel||strcmp(runtime->panel->publication_hash,e->source_hash)||e->scene.pending||e->scene.awaiting_present)return false;
    unsigned outputs=section_count(&e->scene,0,2),controls=section_count(&e->scene,0,1),interactions=section_count(&e->scene,0,3);osg_scene_feedback feedback[64];bool pressed[32]={0};uint8_t record[32];
    for(unsigned i=0;i<outputs;i++){
        if(e->output_widgets[i]>=runtime->panel->widget_count||!osg_scene_record(&e->scene,0,2,i,record,32)){e->failed=true;return false;}
        const osg_sample *sample=&runtime->samples[e->output_widgets[i]];
        bool available=runtime->connected&&sample->available&&sample->source_status!=OSG_FEEDBACK_UNKNOWN&&sample->source_status!=OSG_FEEDBACK_DISCONNECTED;
        bool fresh=available&&sample->source_status!=OSG_FEEDBACK_STALE&&now>=sample->received_ms&&sample->source_age_ms<OSG_STALE_MS&&now-sample->received_ms<OSG_STALE_MS-sample->source_age_ms;
        feedback[i]=(osg_scene_feedback){(osg_expression_type)record[6],available,fresh,record[6]==1?(sample->boolean?1:0):sample->number};
    }
    for(unsigned i=0;i<controls;i++){
        if(!osg_scene_record(&e->scene,0,1,i,record,32)||u16(record+28)>=32){e->failed=true;return false;}
        pressed[u16(record+28)]=runtime->armed&&runtime->active==(int)e->control_widgets[i];
    }
    osg_scene_state state={feedback,(uint16_t)outputs,pressed,(uint16_t)interactions,runtime->connected,runtime->page,now};
    bool ok=osg_scene_update(&e->scene,&state);e->failed=e->scene.failed;e->ready=e->scene.ready;return ok;
}
unsigned osg_engine_render_slice(osg_engine *e,unsigned max_tiles,osg_write_pixels write,void *context){if(!e||!e->active)return 0;unsigned n=osg_scene_render_slice(&e->scene,max_tiles,write,context);e->failed=e->scene.failed;e->ready=e->scene.ready;return n;}
void osg_engine_presented(osg_engine *e,bool success){if(e){osg_scene_presented(&e->scene,success);e->failed=e->scene.failed;e->ready=e->scene.ready;}}
static void clip_rect(int *x,int *y,int *w,int *h,int left,int top,int width,int height){int right=*x+*w,bottom=*y+*h;if(*x<left)*x=left;if(*y<top)*y=top;if(right>left+width)right=left+width;if(bottom>top+height)bottom=top+height;*w=right>*x?right-*x:0;*h=bottom>*y?bottom-*y:0;}
bool osg_engine_copy_hits(osg_engine *e,osg_engine_hit_cache *out){
    if(!e||!out)return false;
    memset(out,0,sizeof(*out));out->page=e->scene.active_page;out->generation=e->scene.generation;
    if(!e->active||e->scene.failed)return false;
    if(!e->scene.ready||!e->scene.connected)return true;
    uint8_t hit[24],node[64],interaction[24];
    for(unsigned i=0;i<e->scene.hits;i++){
        if(!osg_scene_record(&e->scene,1,8,i,hit,24))return false;
        unsigned index=u16(hit+12),owner=u16(hit);if(u16(hit+2)!=e->scene.active_page||!e->scene.properties[e->scene.displayed][index].visible)continue;
        if(!osg_scene_record(&e->scene,1,2,index,node,64)||!osg_scene_record(&e->scene,0,3,owner,interaction,24))return false;
        const osg_scene_properties *property=&e->scene.properties[e->scene.displayed][index];int x=s16(hit+4),y=s16(hit+6),w=u16(hit+8),h=u16(hit+10);
        clip_rect(&x,&y,&w,&h,s16(node+4)+property->dx,s16(node+6)+property->dy,u16(node+8),u16(node+10));clip_rect(&x,&y,&w,&h,s16(node+12),s16(node+14),u16(node+16),u16(node+18));clip_rect(&x,&y,&w,&h,e->profile->x,e->profile->y,e->profile->content_width,e->profile->content_height);
        if(!w||!h)continue;
        if(out->count>=OSG_ENGINE_HITS)return false;
        out->hits[out->count++]=(osg_engine_hit){(uint16_t)owner,u16(hit+2),e->interaction_widgets[owner],interaction[6]==2?u16(interaction+14):OSG_ENGINE_NONE,(int16_t)x,(int16_t)y,(uint16_t)w,(uint16_t)h};
    }
    out->ready=true;return true;
}
