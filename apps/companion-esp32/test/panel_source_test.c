#include "panel_source.h"
#include "scene_bank.h"
#include <assert.h>
#include <math.h>
#include <openssl/sha.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const cJSON *get(const cJSON *v,const char *key) { return cJSON_GetObjectItemCaseSensitive(v,key); }
static const char *str(const cJSON *v,const char *key) { return cJSON_GetStringValue(get(v,key)); }
static bool same(const char *a,const char *b) { return a&&b&&!strcmp(a,b); }
static bool sha(const void *p,size_t n,char out[72]) { unsigned char digest[32];SHA256(p,n,digest);memcpy(out,"sha256:",7);for(unsigned i=0;i<32;i++)(void)snprintf(out+7+i*2,3,"%02x",digest[i]);return true; }
static bool no_sha(const void *p,size_t n,char out[72]) { (void)p;(void)n;(void)out;return false; }
static const osg_crypto crypto={.sha256=sha};
static const osg_panel_source_limits limits={.document_bytes=64*1024,.bitmap_references=32,.width=800,.height=480};
static osg_runtime runtime;
static void fill(void *p,int x,int y,int w,int h,uint16_t color) { (void)p;(void)x;(void)y;(void)w;(void)h;(void)color; }
static void text(void *p,int x,int y,int w,int h,const char *s,uint16_t color) { (void)p;(void)x;(void)y;(void)w;(void)h;(void)s;(void)color; }
static void owned(const osg_widget_arena *arena,const void *p,size_t bytes) {
    assert(p&&(uintptr_t)p>=(uintptr_t)arena->data&&(uintptr_t)p+bytes<=(uintptr_t)arena->data+arena->used);
}
static void owned_string(const osg_widget_arena *arena,const char *s) { assert(s);if(*s)owned(arena,s,strlen(s)+1); }
static void inspect(const osg_widget_arena *arena,const osg_scene *image) {
    owned(arena,image,sizeof(*image));assert(osg_panel_valid(&image->panel));
    owned_string(arena,image->panel.ui_id);owned_string(arena,image->panel.publication_hash);
    if(image->panel.metadata) {
        const osg_panel_metadata *m=image->panel.metadata;owned(arena,m,sizeof(*m));owned(arena,m->groups,m->group_count*sizeof(*m->groups));
        for(size_t i=0;i<m->group_count;i++) { owned_string(arena,m->groups[i].id);owned_string(arena,m->groups[i].label); }
    }
    for(size_t i=0;i<image->panel.page_count;i++) { owned_string(arena,image->pages[i].id);owned_string(arena,image->pages[i].title); }
    for(size_t i=0;i<image->panel.widget_count;i++) {
        const osg_widget *w=&image->widgets[i];owned_string(arena,w->id);owned_string(arena,w->label);owned_string(arena,w->reference);owned_string(arena,w->output_id);owned_string(arena,w->prefix);owned_string(arena,w->suffix);owned_string(arena,w->unit);
        if(w->kind==OSG_IMAGE)owned_string(arena,image->image_hashes[i]);
        if(w->kind!=OSG_COMPONENT)continue;
        const osg_widget_binding *binding=w->component;const osg_widget_component *d=binding->definition;
        owned(arena,binding,sizeof(*binding));owned(arena,d,sizeof(*d));owned(arena,d->layers,d->layer_count*sizeof(*d->layers));owned_string(arena,d->id);owned_string(arena,d->title);
        if(d->input_name)owned_string(arena,d->input_name);
        if(d->navigate_page)owned_string(arena,d->navigate_page);
        for(unsigned p=0;p<d->output_count;p++) { owned_string(arena,d->outputs[p].name);if(d->outputs[p].has_unit)owned_string(arena,d->outputs[p].unit);owned_string(arena,binding->outputs[p].id);owned_string(arena,binding->outputs[p].channel); }
        for(unsigned o=0;o<d->option_count;o++)owned_string(arena,d->option_labels[o]);
        for(unsigned l=0;l<d->layer_count;l++) {
            const osg_widget_layer *layer=&d->layers[l];owned_string(arena,layer->id);if(layer->asset_id)owned_string(arena,layer->asset_id);if(layer->unit)owned_string(arena,layer->unit);
            for(unsigned s=0;s<OSG_WIDGET_SOURCE_STATES;s++)if(layer->paints[s].text)owned_string(arena,layer->paints[s].text);
        }
        if(binding->bitmap_count)owned(arena,binding->bitmaps,binding->bitmap_count*sizeof(*binding->bitmaps));
        for(unsigned b=0;b<binding->bitmap_count;b++) { const osg_bound_bitmap *bitmap=&binding->bitmaps[b];owned_string(arena,bitmap->layer_id);owned_string(arena,bitmap->asset_id);owned_string(arena,bitmap->hash);assert(osg_hash_id(bitmap->hash)&&!bitmap->pixels&&!bitmap->width&&!bitmap->height); }
    }
    assert(osg_init(&runtime,&image->panel,(osg_display){.width=800,.height=480,.fill=fill,.text=text},NULL,NULL));
    assert(!runtime.armed&&!runtime.connected&&runtime.await_release);
}
static cJSON *load(const char *path) {
    FILE *f=fopen(path,"rb");assert(f);assert(!fseek(f,0,SEEK_END));long n=ftell(f);assert(n>0&&!fseek(f,0,SEEK_SET));char *bytes=malloc((size_t)n+1);assert(bytes&&fread(bytes,1,(size_t)n,f)==(size_t)n);fclose(f);bytes[n]=0;
    cJSON *fixture=cJSON_Parse(bytes);free(bytes);assert(fixture);return fixture;
}
static cJSON *publication(const cJSON *document) {
    char *bytes=cJSON_PrintUnformatted(document),hash[72];assert(bytes);assert(sha(bytes,strlen(bytes),hash));
    cJSON *out=cJSON_CreateObject();assert(out);assert(cJSON_AddStringToObject(out,"canonicalDocument",bytes));assert(cJSON_AddStringToObject(out,"publicationHash",hash));assert(cJSON_AddArrayToObject(out,"assets"));cJSON_free(bytes);return out;
}
typedef struct { size_t limit, used, bytes; unsigned allocations, releases; bool fail; void *pointers[2]; } heap_probe;
static void *bank_allocate(void *context,size_t bytes) {
    heap_probe *heap=context;if(heap->fail||bytes>heap->limit-heap->used)return NULL;
    void *p=malloc(bytes);assert(p);unsigned index=0;while(index<2&&heap->pointers[index])index++;assert(index<2);heap->pointers[index]=p;heap->used+=bytes;heap->bytes=bytes;heap->allocations++;return p;
}
static void bank_release(void *context,void *p) {
    heap_probe *heap=context;unsigned index=0;while(index<2&&heap->pointers[index]!=p)index++;assert(index<2);heap->pointers[index]=NULL;heap->used-=heap->bytes;heap->releases++;free(p);
}
static size_t bank_available(void *context) { heap_probe *heap=context;return heap->limit-heap->used; }
static void bank_checks(const cJSON *p,const cJSON *next) {
    heap_probe heap={.limit=384*1024};osg_scene_allocator allocator={&heap,bank_allocate,bank_release,bank_available};const osg_scene_memory memory={192*1024,128,32};
    osg_scene_banks banks;assert(osg_scene_banks_init(&banks,memory,allocator));osg_scene_ticket first,second,stale;osg_scene *scene,*candidate_scene;osg_panel_source_diagnostic error;
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_PREPARED);
    assert(first.slot==0&&first.generation==1&&!banks.active_valid&&heap.allocations==1);
    assert(osg_scene_banks_candidate(&banks,first)==scene&&!osg_scene_banks_activate(&banks,first));
    assert(osg_scene_banks_checked(&banks,first,true)&&osg_scene_banks_activate(&banks,first));
    assert(banks.active_valid&&banks.active==0&&!osg_scene_banks_candidate(&banks,first));
    const size_t bytes=banks.slots[0].arena.used;unsigned char *snapshot=malloc(bytes);assert(snapshot);memcpy(snapshot,banks.slots[0].arena.data,bytes);
    assert(osg_scene_banks_prepare(&banks,&scene->panel,next,str(next,"publicationHash"),crypto,&limits,&second,&candidate_scene,&error)==OSG_SCENE_PREPARED);
    assert(second.slot==1&&heap.allocations==2&&!memcmp(snapshot,banks.slots[0].arena.data,bytes));
    assert(osg_scene_banks_checked(&banks,second,false)&&!osg_scene_banks_candidate(&banks,second)&&banks.active==0);
    const osg_scene_banks before=banks;
    assert(osg_scene_banks_prepare(&banks,&candidate_scene->panel,NULL,NULL,crypto,&limits,&stale,&candidate_scene,&error)==OSG_SCENE_WAIT);
    assert(!memcmp(&before,&banks,sizeof(banks))&&!candidate_scene&&!stale.generation);
    stale=second;assert(osg_scene_banks_prepare(&banks,NULL,next,str(next,"publicationHash"),crypto,&limits,&second,&candidate_scene,&error)==OSG_SCENE_PREPARED);
    assert(second.generation==stale.generation+1&&!osg_scene_banks_checked(&banks,stale,true));
    assert(osg_scene_banks_checked(&banks,second,true)&&osg_scene_banks_activate(&banks,second)&&banks.active==1);
    assert(osg_scene_banks_prepare(&banks,&scene->panel,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_WAIT);
    assert(!memcmp(snapshot,banks.slots[0].arena.data,bytes));free(snapshot);
    const size_t active_bytes=banks.slots[1].arena.used;snapshot=malloc(active_bytes);assert(snapshot);memcpy(snapshot,banks.slots[1].arena.data,active_bytes);
    assert(osg_scene_banks_prepare(&banks,NULL,p,"sha256:0000000000000000000000000000000000000000000000000000000000000000",crypto,&limits,&first,&scene,&error)==OSG_SCENE_REJECTED);
    assert(!scene&&!first.generation&&banks.active==1&&!memcmp(snapshot,banks.slots[1].arena.data,active_bytes));
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_PREPARED);
    stale=first;osg_scene_banks_cancel(&banks,first);assert(!osg_scene_banks_candidate(&banks,first));
    assert(osg_scene_banks_prepare(&banks,&scene->panel,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_WAIT);
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_PREPARED);
    assert(!osg_scene_banks_checked(&banks,stale,true)&&!osg_scene_banks_activate(&banks,stale));
    assert(osg_scene_banks_checked(&banks,first,true)&&osg_scene_banks_activate(&banks,first));
    osg_scene_banks_cancel(&banks,first);assert(banks.active==0&&banks.active_valid);
    banks.slots[1].generation=UINT64_MAX;
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&second,&candidate_scene,&error)==OSG_SCENE_REJECTED&&same(error.field,"sceneBank.generationExhausted"));
    assert(!memcmp(snapshot,banks.slots[1].arena.data,active_bytes));free(snapshot);
    assert(!osg_scene_banks_candidate(NULL,first)&&!osg_scene_banks_candidate(&banks,(osg_scene_ticket){2,1})&&!osg_scene_banks_candidate(&banks,(osg_scene_ticket){0,0}));
    assert(!osg_scene_banks_checked(NULL,first,true)&&!osg_scene_banks_checked(&banks,(osg_scene_ticket){2,1},true)&&!osg_scene_banks_activate(&banks,(osg_scene_ticket){0,0}));
    osg_scene_banks_cancel(NULL,first);osg_scene_banks_destroy(&banks);assert(!heap.used&&heap.releases==2);
    heap.limit=1024;assert(osg_scene_banks_init(&banks,memory,allocator));
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_REJECTED&&same(error.field,"sceneBank.bytes")&&error.required==memory.bytes&&error.available==1024);
    heap.limit=384*1024;heap.fail=true;banks.allocator.available=NULL;
    assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,&error)==OSG_SCENE_REJECTED&&error.available==0);
    heap.fail=false;assert(osg_scene_banks_prepare(&banks,NULL,p,str(p,"publicationHash"),crypto,&limits,&first,&scene,NULL)==OSG_SCENE_PREPARED&&first.generation==3);
    assert(!osg_scene_banks_init(NULL,memory,allocator)&&!osg_scene_banks_init(&banks,(osg_scene_memory){0,1,1},allocator)&&!osg_scene_banks_init(&banks,(osg_scene_memory){1,0,1},allocator)&&!osg_scene_banks_init(&banks,(osg_scene_memory){1,1,0},allocator));
    assert(!osg_scene_banks_init(&banks,memory,(osg_scene_allocator){0})&&!osg_scene_banks_init(&banks,memory,(osg_scene_allocator){.allocate=bank_allocate}));
    assert(osg_scene_banks_prepare(NULL,NULL,p,NULL,crypto,&limits,&first,&scene,&error)==OSG_SCENE_REJECTED);
    assert(osg_scene_banks_prepare(&banks,NULL,p,NULL,crypto,&limits,NULL,&scene,NULL)==OSG_SCENE_REJECTED);
    assert(osg_scene_banks_prepare(&banks,NULL,p,NULL,crypto,&limits,&first,NULL,&error)==OSG_SCENE_REJECTED);
    osg_scene_banks_destroy(&banks);osg_scene_banks_destroy(NULL);assert(!heap.used&&heap.allocations==heap.releases);
    puts("Source banks preserve active/borrowed scenes, fence cancelled candidates, reject stale preflight tickets and recover from allocation failure");
}
static void mutate(cJSON *doc,const cJSON *mutation) {
    const cJSON *path=get(mutation,"path");cJSON *parent=doc;const int length=cJSON_GetArraySize(path);assert(length>0);
    for(int i=0;i<length-1;i++) { const cJSON *key=cJSON_GetArrayItem(path,i);parent=cJSON_IsNumber(key)?cJSON_GetArrayItem(parent,key->valueint):cJSON_GetObjectItemCaseSensitive(parent,cJSON_GetStringValue(key));assert(parent); }
    const cJSON *last=cJSON_GetArrayItem(path,length-1);const bool remove=cJSON_IsTrue(get(mutation,"remove"));
    if(cJSON_IsNumber(last)) {
        if(remove)cJSON_DeleteItemFromArray(parent,last->valueint);else assert(cJSON_ReplaceItemInArray(parent,last->valueint,cJSON_Duplicate(get(mutation,"value"),true)));
    } else {
        const char *key=cJSON_GetStringValue(last);
        if(remove)cJSON_DeleteItemFromObjectCaseSensitive(parent,key);
        else { cJSON *value=cJSON_Duplicate(get(mutation,"value"),true);assert(value);if(get(parent,key))assert(cJSON_ReplaceItemInObjectCaseSensitive(parent,key,value));else assert(cJSON_AddItemToObject(parent,key,value)); }
    }
}
static void rejects(const cJSON *p,const osg_panel_source_limits *profile,osg_widget_arena *arena,osg_panel_source_error code,const char *field) {
    osg_scene *out=(void *)1;const osg_widget_arena mark=*arena;osg_panel_source_diagnostic diagnostic;
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,profile,arena,&out,&diagnostic));assert(!out&&diagnostic.code==code);assert(!memcmp(arena,&mark,sizeof(mark)));
    if(field&&!same(field,diagnostic.field)) { fprintf(stderr,"Expected field %s, got %s\n",field,diagnostic.field);abort(); }
}
static void fault_checks(const cJSON *p,const cJSON *art,osg_widget_arena *arena) {
    const osg_widget_arena mark=*arena;osg_scene *out;osg_panel_source_diagnostic diagnostic;osg_panel_source_limits profile=limits;
    profile.document_bytes=strlen(str(p,"canonicalDocument"))-1;rejects(p,&profile,arena,OSG_PANEL_SOURCE_BUDGET,"documentBytes");
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&profile,arena,&out,&diagnostic));assert(diagnostic.required==profile.document_bytes+1&&diagnostic.available==profile.document_bytes);
    profile=limits;profile.bitmap_references=0;rejects(art,&profile,arena,OSG_PANEL_SOURCE_BUDGET,"bitmapReferences");
    arena->max_layers=1;rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,"layers");*arena=mark;
    arena->max_components=mark.components;rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,"components");*arena=mark;
    for(size_t capacity=mark.used;capacity<mark.used+4096;capacity+=137) { arena->capacity=capacity;rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,NULL);*arena=mark; }
    profile=limits;profile.width=64;profile.height=64;rejects(p,&profile,arena,OSG_PANEL_SOURCE_UNSUPPORTED,"hitArea.minimum44Pixels");
    assert(osg_json_configure(4096,192*1024,8192));rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,"json.heapBytes");
    assert(osg_json_configure(1024*1024,192*1024,1));rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,"json.nodes");
    assert(osg_json_configure(1024*1024,1,8192));rejects(p,&limits,arena,OSG_PANEL_SOURCE_BUDGET,"json.replyBytes");
    assert(osg_json_configure(1024*1024,192*1024,8192));
    assert(!osg_panel_source_compile(NULL,str(p,"publicationHash"),crypto,&limits,arena,&out,NULL)&&!out);
    assert(!osg_panel_source_compile(p,"sha256:0000000000000000000000000000000000000000000000000000000000000000",crypto,&limits,arena,&out,&diagnostic)&&diagnostic.code==OSG_PANEL_SOURCE_IDENTITY);
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),(osg_crypto){.sha256=no_sha},&limits,arena,&out,&diagnostic)&&diagnostic.code==OSG_PANEL_SOURCE_IDENTITY);
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),(osg_crypto){0},&limits,arena,&out,&diagnostic));
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,NULL,&out,&diagnostic));
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,NULL,arena,&out,&diagnostic));
    assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,arena,NULL,&diagnostic));
    const size_t capacity=arena->capacity;arena->used=capacity+1;assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,arena,&out,&diagnostic));*arena=mark;
    arena->data++;assert(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,arena,&out,&diagnostic));*arena=mark;
    for(unsigned i=0;i<6;i++) { profile=limits;if(i==0)profile.document_bytes=0;else if(i==1)profile.document_bytes=1024*1024+1;else if(i==2)profile.width=63;else if(i==3)profile.height=63;else if(i==4)profile.width=4097;else profile.height=4097;rejects(p,&profile,arena,OSG_PANEL_SOURCE_INVALID,"arguments"); }
    cJSON *bad=cJSON_Duplicate(p,true);assert(bad);
    cJSON_ReplaceItemInObjectCaseSensitive(bad,"canonicalDocument",cJSON_CreateString("{}"));rejects(bad,&limits,arena,OSG_PANEL_SOURCE_IDENTITY,"publicationHash");
    cJSON_ReplaceItemInObjectCaseSensitive(bad,"publicationHash",cJSON_CreateString("invalid"));rejects(bad,&limits,arena,OSG_PANEL_SOURCE_IDENTITY,"publicationHash");cJSON_Delete(bad);
    bad=cJSON_Duplicate(p,true);cJSON_DeleteItemFromObjectCaseSensitive(bad,"assets");rejects(bad,&limits,arena,OSG_PANEL_SOURCE_INVALID,"publication.assets");cJSON_Delete(bad);
    cJSON *null=cJSON_CreateNull();bad=publication(null);cJSON_Delete(null);rejects(bad,&limits,arena,OSG_PANEL_SOURCE_INVALID,"document");cJSON_Delete(bad);
    bad=cJSON_Duplicate(p,true);cJSON_ReplaceItemInObjectCaseSensitive(bad,"canonicalDocument",cJSON_CreateString("{"));char hash[72];sha("{",1,hash);cJSON_ReplaceItemInObjectCaseSensitive(bad,"publicationHash",cJSON_CreateString(hash));rejects(bad,&limits,arena,OSG_PANEL_SOURCE_INVALID,"document.json");cJSON_Delete(bad);
}
int main(int argc,char **argv) {
    assert(argc==2);cJSON *fixture=load(argv[1]);const cJSON *cases=get(fixture,"cases");
    osg_json_init();assert(osg_json_configure(1024*1024,192*1024,8192));
    unsigned char *memory=malloc(192*1024);assert(memory);osg_widget_arena arena={.data=memory,.capacity=192*1024,.max_layers=128,.max_components=32};
    osg_scene *first=NULL;size_t total_widgets=0;
    for(const cJSON *test=cases->child;test;test=test->next) {
        osg_scene *image;osg_panel_source_diagnostic diagnostic;const cJSON *p=get(test,"publication");arena.used=arena.layers=arena.components=0;
        if(!osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,&arena,&image,&diagnostic)) { fprintf(stderr,"Positive case failed: %s %s %d\n",str(get(get(test,"document"),"ui"),"id"),diagnostic.field,diagnostic.code);abort(); }
        cJSON *copy=cJSON_Duplicate(p,true);osg_widget_arena temporary=arena;temporary.used=temporary.layers=temporary.components=0;
        osg_scene *owned_image;assert(osg_panel_source_compile(copy,str(copy,"publicationHash"),crypto,&limits,&temporary,&owned_image,NULL));cJSON_Delete(copy);inspect(&temporary,owned_image);
        total_widgets+=image->panel.widget_count;first=image;
    }
    arena.used=arena.layers=arena.components=0;const cJSON *p=get(cJSON_GetArrayItem(cases,0),"publication");
    assert(osg_panel_source_compile(p,str(p,"publicationHash"),crypto,&limits,&arena,&first,NULL));inspect(&arena,first);
    const osg_widget_arena mark=arena;unsigned char *snapshot=malloc(mark.used);assert(snapshot);memcpy(snapshot,memory,mark.used);
    unsigned count=0,valid=0,target_limited=0;
    for(const cJSON *mutation=get(fixture,"mutations")->child;mutation;mutation=mutation->next,count++) {
        const cJSON *test=cJSON_GetArrayItem(cases,get(mutation,"index")->valueint);cJSON *copy=cJSON_Duplicate(get(test,"document"),true);assert(copy);mutate(copy,mutation);cJSON *candidate=publication(copy);cJSON_Delete(copy);
        osg_scene *image;osg_panel_source_diagnostic diagnostic;const bool expected=cJSON_IsTrue(get(mutation,"valid"));
        const bool actual=osg_panel_source_compile(candidate,str(candidate,"publicationHash"),crypto,&limits,&arena,&image,&diagnostic);cJSON_Delete(candidate);
        if((actual&&!expected)||(!actual&&expected&&diagnostic.code!=OSG_PANEL_SOURCE_BUDGET&&diagnostic.code!=OSG_PANEL_SOURCE_UNSUPPORTED)) { fprintf(stderr,"Mutation %u expected %d got %d: code %d field %s widget %s\n",count,expected,actual,diagnostic.code,diagnostic.field,diagnostic.widget);abort(); }
        if(actual) { inspect(&arena,image);valid++; } else { assert(!image&&diagnostic.code!=OSG_PANEL_SOURCE_OK);assert(!memcmp(&arena,&mark,sizeof(mark))); }
        if(expected&&!actual) { target_limited++;printf("Shared-valid mutation %u: explicit target rejection %s (%zu required, %zu available)\n",count,diagnostic.field,diagnostic.required,diagnostic.available); }
        assert(!memcmp(snapshot,memory,mark.used));arena=mark;assert(osg_json_allocated()==0);
    }
    fault_checks(p,get(cJSON_GetArrayItem(cases,3),"publication"),&arena);assert(!memcmp(snapshot,memory,mark.used));free(snapshot);free(memory);assert(osg_json_allocated()==0);
    bank_checks(p,get(cJSON_GetArrayItem(cases,1),"publication"));assert(osg_json_allocated()==0);
    const int case_count=cJSON_GetArraySize(cases);cJSON_InitHooks(NULL);cJSON_Delete(fixture);
    printf("Complete panel source admission: %d cases, %zu widget instances, %u shared mutations (%u admitted, %u explicitly target-limited); immutable arena prefix %zu bytes\n",case_count,total_widgets,count,valid,target_limited,mark.used);return 0;
}
