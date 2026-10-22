#include "panel_draw.h"
#include "panel_source.h"
#include <assert.h>
#include <math.h>
#include <openssl/sha.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint16_t pixels[800*480],before[800*480],buffer[800*4] __attribute__((aligned(64)));
static osg_panel_draw draw;
static osg_runtime runtime;
static unsigned writes,fences;
static bool fail_write,fail_present;
static bool write_pixels(void *context,int x,int y,int w,int h,const uint16_t *data) {
    (void)context;assert(x>=0&&y>=0&&w>0&&h>0&&x+w<=800&&y+h<=480&&w*h<=800*4);
    for(int row=0;row<h;row++)memcpy(pixels+(y+row)*800+x,data+row*w,(size_t)w*2);
    writes++;return !fail_write;
}
static bool present(void *context) { (void)context;fences++;return !fail_present; }
static bool sha(const void *data,size_t size,char out[72]) {
    unsigned char hash[32];SHA256(data,size,hash);memcpy(out,"sha256:",7);
    for(unsigned i=0;i<32;i++)(void)snprintf(out+7+i*2,3,"%02x",hash[i]);
    return true;
}
static cJSON *load(const char *path) {
    FILE *f=fopen(path,"rb");assert(f&&!fseek(f,0,SEEK_END));long size=ftell(f);assert(size>0&&!fseek(f,0,SEEK_SET));
    char *data=malloc((size_t)size+1);assert(data&&fread(data,1,(size_t)size,f)==(size_t)size);fclose(f);data[size]=0;
    cJSON *result=cJSON_Parse(data);free(data);assert(result);return result;
}
static osg_display display(void) { return (osg_display){.width=800,.height=480}; }
static void full(void) {
    osg_display original=runtime.display;assert(osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100));
    assert(!memcmp(&original,&runtime.display,sizeof(original))&&!draw.frame&&!draw.layer);
}
static void mixed_order(void) {
    osg_widget_layer red={.id="base",.kind=OSG_LAYER_RECTANGLE,.bounds={0,0,1,1},.output=-1};
    osg_widget_layer blue=red;blue.id="top";
    for(unsigned s=0;s<8;s++) { red.paints[s]=(osg_widget_paint){.visible=true,.opacity=1,.fill=0xff0000,.stroke=UINT32_MAX};blue.paints[s]=red.paints[s];blue.paints[s].fill=0x0000ff; }
    osg_widget_component definitions[2]={{.id="red",.width=200,.height=120,.layers=&red,.layer_count=1,.confirmed_output=-1,.warning_output=-1,.enabled_output=-1},
        {.id="blue",.width=200,.height=120,.layers=&blue,.layer_count=1,.confirmed_output=-1,.warning_output=-1,.enabled_output=-1}};
    osg_widget_binding bindings[2]={{.definition=&definitions[0]},{.definition=&definitions[1]}};
    const unsigned char green[]={0xe0,0x07,128};
    osg_widget widgets[]={
        {.id="back",.page=0,.kind=OSG_COMPONENT,.bounds={.1,.1,.4,.4},.component=&bindings[0]},
        {.id="image",.page=0,.kind=OSG_IMAGE,.bounds={.2,.2,.4,.4},.pixels=green,.image_width=1,.image_height=1},
        {.id="front",.page=0,.kind=OSG_COMPONENT,.bounds={.3,.3,.1,.1},.component=&bindings[1]},
        {.id="legacy",.page=0,.kind=OSG_TEXT,.label="Legacy",.prefix="",.suffix="",.unit="",.reference="status",.output_id="status",.bounds={.7,.1,.25,.25},.background=0xffff,.foreground=0x0000}
    };
    const osg_page page={"main","Main",800,480};const osg_widget_group group={"group","SYSTEMS",0,15};const osg_panel_metadata metadata={&group,1};
    const osg_panel panel={"mixed","test",&page,widgets,1,4,&metadata};
    memset(&runtime,0,sizeof(runtime));runtime.panel=&panel;runtime.display=display();runtime.active=-1;runtime.connected=true;
    assert(osg_panel_draw_check(&draw,&runtime,100,true));full();
    assert(pixels[60*800+90]==0xf800); /* Base is behind the later alpha image. */
    uint16_t blend=pixels[110*800+180];assert((blend>>11)>=14&&(blend>>11)<=16&&((blend>>5)&63)>=30&&((blend>>5)&63)<=33);
    assert(pixels[160*800+280]==0x001f&&pixels[55*800+570]==0xffff);
    unsigned count=writes;memcpy(before,pixels,sizeof(pixels));
    assert(osg_panel_draw_render(&draw,&runtime,(osg_damage){0},100)&&writes==count);
    blue.paints[OSG_WIDGET_NORMAL].fill=0xffffff;
    bool partial=osg_panel_draw_render(&draw,&runtime,(osg_damage){.widgets=4},100);
    if(!partial)fprintf(stderr,"partial: code=%d widget=%s layer=%s field=%s required=%zu available=%zu writes=%u flushes=%u\n",draw.diagnostic.code,draw.diagnostic.widget,draw.diagnostic.layer,draw.diagnostic.field,draw.diagnostic.required,draw.diagnostic.available,writes,draw.backend.flushes);
    assert(partial);
    assert(pixels[160*800+280]==0xffff&&writes>count);
    for(int y=0;y<480;y++)for(int x=0;x<800;x++)if(x<240||x>=320||y<144||y>=192)assert(pixels[y*800+x]==before[y*800+x]);
    count=writes;widgets[1].pixels=NULL;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count);
    assert(draw.diagnostic.code==OSG_WIDGET_DRAW_ASSET&&!strcmp(draw.diagnostic.widget,"image"));widgets[1].pixels=green;
    runtime.screen=OSG_SCREEN_ERROR;full();runtime.screen=OSG_SCREEN_PANEL;
    fail_write=true;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100));fail_write=false;
    fail_present=true;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100));fail_present=false;full();
    definitions[0].width=1;assert(!osg_panel_draw_check(&draw,&runtime,100,true)&&!strcmp(draw.diagnostic.field,"scale"));definitions[0].width=200;
    widgets[0].component=NULL;assert(!osg_panel_draw_check(&draw,&runtime,100,true));widgets[0].component=&bindings[0];
    osg_widget_group broken[]={group,{"empty","Empty",0,0},{"other","Other",1,15}};
    osg_panel_metadata extra={broken,3};osg_panel extra_panel=panel;extra_panel.metadata=&extra;runtime.panel=&extra_panel;full();
    broken[0].label="\xf0\x9f\x9b\xa9";count=writes;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count&&!strcmp(draw.diagnostic.field,"group.label"));
    extra_panel.metadata=NULL;strcpy(runtime.screen_message,"Panel budget exceeded: previous panel retained disarmed");full();assert(pixels[479*800+799]==0x3146);runtime.screen_message[0]=0;
}
static void catalog(const cJSON *fixture,const char *directory) {
    const cJSON *cases=cJSON_GetObjectItemCaseSensitive(fixture,"cases");
    const int admitted[]={0,1,6};
    for(size_t selected=0;selected<sizeof(admitted)/sizeof(*admitted);selected++) {
        const int n=admitted[selected];
        const cJSON *publication=cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(cases,n),"publication");
        unsigned char *memory=malloc(192*1024);assert(memory);osg_widget_arena arena={.data=memory,.capacity=192*1024,.max_layers=128,.max_components=32};
        osg_scene *scene=NULL;osg_panel_source_diagnostic diagnostic;
        const osg_panel_source_limits limits={.document_bytes=65536,.bitmap_references=32,.width=800,.height=480};
        assert(osg_panel_source_compile(publication,osg_string(publication,"publicationHash"),(osg_crypto){.sha256=sha},&limits,&arena,&scene,&diagnostic));
        /* Neither renderer initialization nor source admission creates authority. */
        memset(&runtime,0,sizeof(runtime));runtime.panel=&scene->panel;runtime.display=display();runtime.active=-1;
        assert(osg_panel_draw_check(&draw,&runtime,100,true));full();assert(!runtime.armed&&!runtime.connected);
        for(size_t slot=0;slot<32;slot++) {
            osg_output_contract contract;if(!osg_panel_output(runtime.panel,slot,&contract))continue;
            runtime.samples[slot]=(osg_sample){.available=true,.received=true,.simulator=true,.kind=contract.kind,.provenance=OSG_GAME_CONFIRMED,.received_ms=100,.boolean=true,.number=contract.has_range?(contract.min+contract.max)/2:42};
            strcpy(runtime.samples[slot].text,"ONLINE");
        }
        runtime.connected=true;runtime.armed=true;full();
        for(size_t p=0;p<scene->panel.page_count;p++) { runtime.page=(uint8_t)p;full(); }
        runtime.page=0;assert(osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},1100));
        if(n==0||n==6) {
            full();char path[1024];assert(snprintf(path,sizeof(path),"%s/panel-catalog%s.ppm",directory,n==6?"-complete":"")>0);FILE *f=fopen(path,"wb");assert(f);fprintf(f,"P6\n800 480\n255\n");
            for(unsigned i=0;i<800*480;i++){uint32_t c=pixels[i];unsigned char rgb[]={(unsigned char)((c>>11)*255/31),(unsigned char)(((c>>5)&63)*255/63),(unsigned char)((c&31)*255/31)};assert(fwrite(rgb,1,3,f)==3);}fclose(f);
        }
        free(memory);
    }
}
static void preflight(void) {
    osg_widget_layer layer={.id="value",.kind=OSG_LAYER_TEXT,.bounds={0,0,1,1},.output=0,.font=OSG_FONT_MONO,.font_size=22,.align=0,.precision=1};
    for(unsigned s=0;s<8;s++)layer.paints[s]=(osg_widget_paint){.visible=true,.opacity=1,.fill=0xffffff,.text="VALUE"};
    osg_widget_component definition={.id="dynamic",.width=200,.height=120,.layers=&layer,.layer_count=1,.output_count=1,.confirmed_output=-1,.warning_output=-1,.enabled_output=-1};
    osg_widget_binding binding={.definition=&definition,.outputs={{.id="value",.channel="value",.slot=0}}};
    const osg_widget widget={.id="dynamic-instance",.page=0,.kind=OSG_COMPONENT,.bounds={.1,.1,.4,.4},.component=&binding};
    const osg_page page={"main","Main",800,480};const osg_panel panel={"dynamic","test",&page,&widget,1,1,NULL};
    memset(&runtime,0,sizeof(runtime));runtime.panel=&panel;runtime.display=display();runtime.active=-1;runtime.connected=true;
    runtime.samples[0]=(osg_sample){.available=true,.received=true,.simulator=true,.kind=OSG_NUMBER,.provenance=OSG_GAME_CONFIRMED,.received_ms=100,.number=1e21};
    unsigned count=writes;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count);
    assert(draw.diagnostic.code==OSG_WIDGET_DRAW_TEXT&&!strcmp(draw.diagnostic.widget,"dynamic-instance")&&!strcmp(draw.diagnostic.layer,"value"));
    char unit[241];for(unsigned i=0;i<120;i++){unit[2*i]=(char)0xc3;unit[2*i+1]=(char)0xa9;}unit[240]=0;
    layer.unit=unit;runtime.samples[0].number=42;assert(osg_panel_draw_check(&draw,&runtime,100,true));full();
    draw.max_font_size=16;count=writes;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count);
    assert(draw.diagnostic.code==OSG_WIDGET_DRAW_BUDGET&&!strcmp(draw.diagnostic.field,"fontSize")&&draw.diagnostic.required==22&&draw.diagnostic.available==16);draw.max_font_size=128;
    runtime.samples[0].kind=OSG_STRING;strcpy(runtime.samples[0].text,"\xf0\x9f\x9b\xa9");count=writes;
    assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count);
    layer.output=-1;layer.paints[OSG_WIDGET_WARNING].text="\xf0\x9f\x9b\xa9";
    assert(osg_panel_draw_check(&draw,&runtime,100,false)&&!osg_panel_draw_check(&draw,&runtime,100,true));
    layer.kind=OSG_LAYER_IMAGE;layer.asset_id="asset";count=writes;
    assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100)&&writes==count&&!strcmp(draw.diagnostic.field,"asset"));
    const unsigned char image[]={0xff,0xff,255};osg_bound_bitmap prepared={.layer_id="value",.asset_id="asset",.pixels=image,.width=320,.height=192};
    /* Preflight checks prepared dimensions; real cache pixels have that extent. */
    unsigned char *large=malloc((size_t)320*192*3);assert(large);for(size_t i=0;i<(size_t)320*192;i++)memcpy(large+i*3,image,3);prepared.pixels=large;
    binding.bitmaps=&prepared;binding.bitmap_count=1;full();free(large);
    runtime.display.width=480;assert(!osg_panel_draw_check(&draw,&runtime,100,false));runtime.display.width=800;
    runtime.page=1;assert(!osg_panel_draw_check(&draw,&runtime,100,true));runtime.page=0;
    draw.backend.rendering=true;assert(!osg_panel_draw_render(&draw,&runtime,(osg_damage){.full=true},100));osg_panel_draw_destroy(&draw);assert(draw.backend.display);draw.backend.rendering=false;
}
static void compatibility(const cJSON *fixture) {
    unsigned accepted=0,rejected_text=0;
    const cJSON *cases=cJSON_GetObjectItemCaseSensitive(fixture,"cases"),*profiles=cJSON_GetObjectItemCaseSensitive(fixture,"profiles");
    for(const cJSON *item=cases->child;item;item=item->next) {
        const cJSON *issues=cJSON_GetObjectItemCaseSensitive(item,"issues"),*profile=cJSON_GetArrayItem(profiles,cJSON_GetObjectItemCaseSensitive(item,"profile")->valueint);
        const cJSON *limits=cJSON_GetObjectItemCaseSensitive(profile,"limits");
        const char *name=osg_string(item,"name"),*canonical=osg_string(item,"canonicalDocument");
        bool check_text=false,check_zero=false;
        for(const cJSON *issue=issues->child;issue;issue=issue->next) {
            const char *field=osg_string(issue,"field");
            if(!strcmp(field,"text.glyphs")||!strcmp(field,"group.label")||!strcmp(field,"fontSize"))check_text=true;
            if(!strcmp(field,"geometry.minimumPixels"))check_zero=true;
        }
        if(issues->child&&!check_text&&!check_zero)continue;
        size_t capacity=(size_t)cJSON_GetObjectItemCaseSensitive(limits,"sceneBytes")->valueint;
        unsigned char *memory=malloc(capacity);assert(memory);
        osg_widget_arena arena={.data=memory,.capacity=capacity,.max_layers=(size_t)cJSON_GetObjectItemCaseSensitive(limits,"totalLayers")->valueint,.max_components=(size_t)cJSON_GetObjectItemCaseSensitive(limits,"components")->valueint};
        osg_panel_source_limits source_limits={.document_bytes=(size_t)cJSON_GetObjectItemCaseSensitive(limits,"documentBytes")->valueint,.bitmap_references=32,.width=800,.height=480};
        assert(osg_json_configure((size_t)cJSON_GetObjectItemCaseSensitive(limits,"jsonBytes")->valueint,(size_t)cJSON_GetObjectItemCaseSensitive(limits,"replyBytes")->valueint,(size_t)cJSON_GetObjectItemCaseSensitive(limits,"jsonNodes")->valueint));
        char hash[72];assert(sha(canonical,strlen(canonical),hash));cJSON *publication=cJSON_CreateObject();assert(publication);
        assert(cJSON_AddStringToObject(publication,"canonicalDocument",canonical)&&cJSON_AddStringToObject(publication,"publicationHash",hash)&&cJSON_AddArrayToObject(publication,"assets"));
        osg_scene *scene=NULL;osg_panel_source_diagnostic error;
        bool admitted=osg_panel_source_compile(publication,hash,(osg_crypto){.sha256=sha},&source_limits,&arena,&scene,&error);
        if(!admitted)fprintf(stderr,"compatibility %s: source field=%s required=%zu available=%zu\n",name,error.field,error.required,error.available);
        assert(admitted);cJSON_Delete(publication);
        void *prepared[32]={0};unsigned count=0;
        for(size_t i=0;i<scene->panel.widget_count;i++) {
            osg_widget *widget=&scene->widgets[i];int x,y,width,height;osg_panel_widget_rect(&scene->panel,i,800,480,&x,&y,&width,&height);
            if(widget->kind==OSG_IMAGE){
                assert(count<32&&width>0&&height>0);prepared[count]=calloc((size_t)width*height,3);assert(prepared[count]);widget->pixels=prepared[count++];widget->image_width=(uint16_t)width;widget->image_height=(uint16_t)height;
            }else if(widget->kind==OSG_COMPONENT){
                for(unsigned b=0;b<widget->component->bitmap_count;b++){
                    osg_bound_bitmap *bitmap=&widget->component->bitmaps[b];bitmap->width=(uint16_t)lround(width*bitmap->bounds.width);bitmap->height=(uint16_t)lround(height*bitmap->bounds.height);
                    assert(count<32&&bitmap->width&&bitmap->height);prepared[count]=calloc((size_t)bitmap->width*bitmap->height,3);assert(prepared[count]);bitmap->pixels=prepared[count++];
                }
            }
        }
        memset(&runtime,0,sizeof(runtime));runtime.panel=&scene->panel;runtime.display=display();runtime.active=-1;draw.max_font_size=(uint8_t)cJSON_GetObjectItemCaseSensitive(limits,"fontSize")->valueint;
        unsigned previous=writes;bool ready=osg_panel_draw_check(&draw,&runtime,100,true);
        if(check_text||check_zero){assert(!ready&&writes==previous);rejected_text++;}
        else {
            if(!ready)fprintf(stderr,"compatibility %s: preflight field=%s widget=%s layer=%s\n",name,draw.diagnostic.field,draw.diagnostic.widget,draw.diagnostic.layer);
            assert(ready);for(size_t page=0;page<scene->panel.page_count;page++){runtime.page=(uint8_t)page;full();}accepted++;
        }
        for(unsigned i=0;i<count;i++)free(prepared[i]);
        free(memory);
        assert(!runtime.armed&&!runtime.connected);
    }
    draw.max_font_size=128;
    printf("Shared target oracle: %u accepted source panels render, %u glyph/font/zero-area failures reject before writes\n",accepted,rejected_text);
}
int main(int argc,char **argv) {
    assert(argc==3||argc==4);osg_board board={.width=800,.height=480,.write=write_pixels,.present=present};
    assert(!osg_panel_draw_init(NULL,board,buffer,sizeof(buffer)));assert(osg_panel_draw_init(&draw,board,buffer,sizeof(buffer)));
    assert(!osg_panel_draw_check(NULL,NULL,0,true));assert(!osg_panel_draw_check(&draw,NULL,0,true));
    assert(!osg_panel_draw_render(NULL,NULL,(osg_damage){.full=true},0));
    cJSON *fixture=load(argv[1]);catalog(fixture,argv[2]);cJSON_Delete(fixture);mixed_order();preflight();
    if(argc==4){fixture=load(argv[3]);compatibility(fixture);cJSON_Delete(fixture);}
    lv_mem_monitor_t memory;lv_mem_monitor(&memory);assert(memory.free_size&&memory.free_biggest_size);
    printf("LVGL pool=%u bytes; peak-used=%u; free=%u; biggest-free=%u; strips<=%u rows\n",(unsigned)LV_MEM_SIZE,(unsigned)memory.max_used,(unsigned)memory.free_size,(unsigned)memory.free_biggest_size,OSG_WIDGET_DRAW_ROWS);
    osg_panel_draw_destroy(NULL);osg_panel_draw_destroy(&draw);osg_panel_draw_destroy(&draw);
    puts("Whole-panel LVGL: admitted catalogs, group labels, source order, alpha, legacy widgets, clipped damage, zero-write preflight rejection and flush recovery passed.");return 0;
}
