#include "companion.h"
#include "raster.h"
#include "esp_lcd_bridge.h"
#include "panel.h"
#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct { int draws, texts, edges; bool fail; uint64_t sequence; char log[8192]; int width, height; int text_x,text_y,text_width,text_height; } spy;
static void rect(void *ctx, int x, int y, int w, int h, uint16_t c) {
    spy *s = ctx; (void)c;
    assert(x >= 0 && y >= 0 && w > 0 && h > 0 && x + w <= s->width && y + h <= s->height); s->draws++;
}
static void label(void *ctx, int x, int y, int w, int h, const char *text, uint16_t c) {
    spy *s = ctx; rect(ctx, x, y, w, h, c); s->texts++;
    s->text_x=x;s->text_y=y;s->text_width=w;s->text_height=h;
    if (strlen(s->log) + strlen(text) + 2 < sizeof(s->log)) { strcat(s->log, text); strcat(s->log, "\n"); }
}
static bool record(void *ctx, const char *id, bool down, uint64_t sequence) {
    spy *s = ctx; assert(!strcmp(id, "master")); (void)down;
    if (s->fail) return false;
    assert(sequence == s->sequence + 1); s->sequence = sequence; s->edges++; return true;
}
static bool bridge_error, bridge_wait_error;
static int bridge_calls, bridge_waits;
int esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t panel, int x, int y, int right, int bottom, const void *pixels) {
    assert(panel && x == 2 && y == 3 && right == 6 && bottom == 4 && pixels);
    bridge_calls++; return bridge_error ? -1 : ESP_OK;
}
static bool wait_complete(void *ctx) { assert(ctx); bridge_waits++; return !bridge_wait_error; }
static void bridge_test(void) {
    uint16_t data[4] = {0}; int context = 0;
    osg_lcd_bridge bridge = {.panel = &context, .context = &context, .wait_complete = wait_complete};
    assert(!osg_lcd_write(NULL, 2, 3, 4, 1, data));
    assert(!osg_lcd_write(&bridge, 2, 3, 4, 1, NULL));
    assert(!osg_lcd_write(&bridge, 2, 3, 0, 1, data));
    assert(!osg_lcd_write(&bridge, 2, 3, 4, 0, data));
    bridge.panel = NULL; assert(!osg_lcd_write(&bridge, 2, 3, 4, 1, data)); bridge.panel = &context;
    bridge.wait_complete = NULL; assert(!osg_lcd_write(&bridge, 2, 3, 4, 1, data)); bridge.wait_complete = wait_complete;
    bridge_error = true; assert(!osg_lcd_write(&bridge, 2, 3, 4, 1, data)); assert(bridge_waits == 0);
    bridge_error = false; bridge_wait_error = true; assert(!osg_lcd_write(&bridge, 2, 3, 4, 1, data));
    bridge_wait_error = false; assert(osg_lcd_write(&bridge, 2, 3, 4, 1, data)); assert(bridge_calls == 3 && bridge_waits == 2);
}
static osg_runtime runtime;
static osg_runtime *setup(spy *s, int width, int height) {
    *s = (spy){.width = width, .height = height};
    assert(osg_init(&runtime, &osg_demo_panel, (osg_display){s, (uint16_t)width, (uint16_t)height, rect, label,NULL}, record, s));
    return &runtime;
}
static void validation(void) {
    assert(osg_panel_valid(&osg_demo_panel)); assert(!osg_panel_valid(NULL));
    osg_panel p = osg_demo_panel;
    p.ui_id = "!"; assert(!osg_panel_valid(&p)); p = osg_demo_panel;
    const char *hashes[] = {NULL, "sha256:no", "SHA256:af25b3e73156cc526865807484694b70dacbd51306b58b9d4bc0786af322b093", "sha256:zf25b3e73156cc526865807484694b70dacbd51306b58b9d4bc0786af322b093"};
    for (size_t i = 0; i < sizeof(hashes)/sizeof(hashes[0]); i++) { p.publication_hash = hashes[i]; assert(!osg_panel_valid(&p)); }
    p = osg_demo_panel; p.page_count = 0; assert(!osg_panel_valid(&p)); p.page_count = 5; assert(!osg_panel_valid(&p));
    p = osg_demo_panel; p.widget_count = 0; assert(!osg_panel_valid(&p)); p.widget_count = 33; assert(!osg_panel_valid(&p));
    p = osg_demo_panel; p.pages = NULL; assert(!osg_panel_valid(&p)); p = osg_demo_panel; p.widgets = NULL; assert(!osg_panel_valid(&p));
    osg_page pages[2]; memcpy(pages, osg_demo_pages, sizeof(pages)); p = osg_demo_panel; p.pages = pages;
    pages[0].title = ""; assert(!osg_panel_valid(&p)); pages[0] = osg_demo_pages[0];
    pages[0].width = 0; assert(!osg_panel_valid(&p)); pages[0] = osg_demo_pages[0];
    pages[0].height = 9000; assert(!osg_panel_valid(&p)); pages[0] = osg_demo_pages[0];
    pages[0].title = "bad\n"; assert(!osg_panel_valid(&p)); pages[0] = osg_demo_pages[0];
    pages[1].id = pages[0].id; assert(!osg_panel_valid(&p));
    osg_widget widgets[5]; p = osg_demo_panel; p.widgets = widgets;
    for (int field = 0; field < 24; field++) {
        memcpy(widgets, osg_demo_widgets, sizeof(widgets));
        switch (field) {
            case 0: widgets[0].id = NULL; break;
            case 1: widgets[0].reference = "bad id"; break;
            case 2: widgets[0].label = ""; break;
            case 3: widgets[0].prefix = NULL; break;
            case 4: widgets[0].suffix = "\x7f"; break;
            case 5: widgets[0].unit = "012345678901234567890123456789012"; break;
            case 6: widgets[0].page = 2; break;
            case 7: widgets[0].kind = (osg_kind)-1; break;
            case 8: widgets[0].kind = (osg_kind)99; break;
            case 9: widgets[0].precision = 5; break;
            case 10: widgets[0].bounds.x = NAN; break;
            case 11: widgets[0].bounds.y = INFINITY; break;
            case 12: widgets[0].bounds.width = -1; break;
            case 13: widgets[0].bounds.height = 0; break;
            case 14: widgets[0].bounds.x = -1; break;
            case 15: widgets[0].bounds.y = -1; break;
            case 16: widgets[0].bounds.width = 2; break;
            case 17: widgets[0].bounds.height = 2; break;
            case 18: widgets[1].output_id = "!"; break;
            case 19: widgets[3].min = NAN; break;
            case 20: widgets[3].max = -1; break;
            case 21: widgets[1].id = widgets[0].id; break;
            case 22: widgets[1].bounds = widgets[0].bounds; break;
            case 23: widgets[1].kind = OSG_BUTTON; widgets[1].reference = widgets[0].reference; break;
        }
        assert(!osg_panel_valid(&p));
    }
    char long_text[130]; memset(long_text, 'a', 129); long_text[129] = 0;
    memcpy(widgets, osg_demo_widgets, sizeof(widgets)); widgets[0].label = long_text; assert(!osg_panel_valid(&p));
    spy s; osg_runtime *r = setup(&s, 800, 480);
    osg_display bad = r->display; bad.fill = NULL; assert(!osg_init(r, &osg_demo_panel, bad, record, &s));
    bad = r->display; bad.width = 1; assert(!osg_init(r, &osg_demo_panel, bad, record, &s));
    assert(!osg_init(NULL, &osg_demo_panel, r->display, record, &s));
}
static void input(void) {
    spy s; osg_runtime *r = setup(&s, 800, 480);
    assert(!osg_preview_arm(r)); osg_touch(r, true, 100, 100); assert(!osg_preview_arm(r));
    osg_touch(r, false, 0, 0); assert(osg_preview_arm(r));
    osg_touch(r, true, 100, 100); assert(s.edges == 1 && r->active == 0);
    osg_touch(r, true, 100, 100); assert(s.edges == 1);
    osg_touch(r, false, 100, 100); assert(s.edges == 2);
    osg_touch(r, true, 100, 100); assert(s.edges == 3);
    osg_touch(r, true, -1, -1); assert(!r->armed && s.edges == 4);
    osg_touch(r, true, 100, 100); assert(s.edges == 4); osg_touch(r, false, 0, 0);
    assert(osg_preview_arm(r)); osg_touch(r, true, 100, 100); assert(osg_set_page(r, 1)); assert(s.edges == 6 && !r->armed);
    assert(!osg_set_page(r, 2)); osg_touch(r, false, 0, 0); assert(osg_preview_arm(r)); osg_touch(r, true, 100, 100); assert(s.edges == 6);
    osg_touch(r, false, 0, 0); assert(osg_set_page(r, 0)); osg_touch(r, false, 0, 0); assert(osg_preview_arm(r));
    s.fail = true; osg_touch(r, true, 100, 100); assert(!r->armed && r->active < 0);
    s.fail = false; osg_touch(r, false, 0, 0); assert(osg_preview_arm(r)); osg_touch(r, true, 100, 100);
    s.fail = true; osg_touch(r, false, 0, 0); assert(!r->armed);
    r = setup(&s, 800, 480); r->sink = NULL; osg_touch(r, false, 0, 0); assert(!osg_preview_arm(r));
    r = setup(&s, 800, 480); r->sequence = UINT64_MAX; osg_touch(r, false, 0, 0); assert(!osg_preview_arm(r));
    r = setup(&s, 64, 64); osg_touch(r, false, 0, 0); assert(osg_preview_arm(r)); osg_touch(r, true, 10, 30); assert(!s.edges);
    r = setup(&s, 800, 480); osg_touch(r, false, 0, 0); assert(osg_preview_arm(r)); osg_touch(r, true, 790, 470); osg_touch(r, true, 100, 100); assert(!s.edges);
}
static void feedback(void) {
    spy s; osg_runtime *r = setup(&s, 800, 480);
    osg_sample value = {.available = true, .kind = OSG_NUMBER, .provenance = OSG_GAME_CONFIRMED, .number = 125.5};
    assert(!osg_feedback(r, "speed-channel", &value, 50)); osg_connected(r, true); osg_connected(r, true);
    assert(!osg_feedback(r, NULL, &value, 50)); assert(!osg_feedback(r, "speed-channel", NULL, 50));
    value.provenance = (osg_provenance)99; assert(!osg_feedback(r, "speed-channel", &value, 50)); value.provenance = OSG_GAME_CONFIRMED;
    value.source_age_ms = 86400001; assert(!osg_feedback(r, "speed-channel", &value, 50)); value.source_age_ms = 0;
    value.number = NAN; assert(!osg_feedback(r, "speed-channel", &value, 50)); value.number = 601; assert(!osg_feedback(r, "speed-channel", &value, 50));
    value.number = -1; assert(!osg_feedback(r, "speed-channel", &value, 50)); value.number = 125.5;
    value.kind = OSG_STRING; assert(!osg_feedback(r, "speed-channel", &value, 50)); value.kind = OSG_NUMBER;
    assert(!osg_feedback(r, "missing-channel", &value, 50)); assert(osg_feedback(r, "speed-channel", &value, 50));
    osg_render(r, 100); assert(strstr(s.log, "125.5 kt") && strstr(s.log, "GAME CONFIRMED | FRESH") && strstr(s.log, "UNKNOWN"));
    s.log[0] = 0; osg_render(r, 1050); assert(strstr(s.log, "STALE"));
    s.log[0] = 0; osg_render(r, 40); assert(strstr(s.log, "STALE"));
    value.source_age_ms = 1000; assert(osg_feedback(r, "speed-channel", &value, 100)); s.log[0] = 0; osg_render(r, 100); assert(strstr(s.log, "STALE"));
    value.source_age_ms = 500; assert(osg_feedback(r, "speed-channel", &value, 100)); s.log[0] = 0; osg_render(r, 600); assert(strstr(s.log, "STALE"));
    value.kind = OSG_BOOL; value.boolean = true; value.provenance = OSG_LOCAL; value.source_age_ms = 0;
    assert(osg_feedback(r, "caution-channel", &value, 50)); value.kind = OSG_NUMBER; assert(!osg_feedback(r, "caution-channel", &value, 50));
    value.kind = OSG_STRING; strcpy(value.text, "123.450"); assert(osg_feedback(r, "radio-channel", &value, 50));
    value.kind = OSG_NUMBER; assert(!osg_feedback(r, "radio-channel", &value, 50)); value.kind = OSG_STRING;
    memset(value.text, 'A', sizeof(value.text)); assert(!osg_feedback(r, "radio-channel", &value, 50));
    strcpy(value.text, "bad\n"); assert(!osg_feedback(r, "radio-channel", &value, 50));
    assert(osg_set_page(r, 1)); value.kind = OSG_NUMBER; value.number = 75; assert(osg_feedback(r, "rpm-channel", &value, 50)); osg_render(r, 100);
    osg_render(r, 1050); osg_connected(r, false); assert(!r->samples[4].available && !r->armed);
    s.log[0] = 0; osg_render(r, 1050); assert(strstr(s.log, "DISCONNECTED"));
}
static uint16_t *pixels;
static int screen_width, screen_height;
static bool fail_write;
static bool write_pixels(void *ctx, int x, int y, int w, int h, const uint16_t *p) {
    (void)ctx;
    assert(x >= 0 && y >= 0 && w > 0 && h == 1 && x + w <= screen_width && y + h <= screen_height);
    if (fail_write) return false;
    memcpy(&pixels[y * screen_width + x], p, (size_t)w * sizeof(*p)); return true;
}
static void preview(const char *path, int width, int height, osg_screen_state screen) {
    screen_width = width; screen_height = height; pixels = calloc((size_t)width * height, sizeof(*pixels)); assert(pixels);
    osg_raster raster = {.write = write_pixels};
    assert(osg_init(&runtime, &osg_demo_panel, osg_raster_display(&raster, (uint16_t)width, (uint16_t)height), NULL, NULL));
    osg_connected(&runtime, true);
    osg_sample sample = {.available = true, .kind = OSG_STRING, .provenance = OSG_LOCAL}; strcpy(sample.text, "123.450");
    assert(osg_feedback(&runtime, "radio-channel", &sample, 0)); sample.kind = OSG_NUMBER; sample.number = 125.5;
    assert(osg_feedback(&runtime, "speed-channel", &sample, 0)); sample.kind = OSG_BOOL; sample.boolean = true;
    assert(osg_feedback(&runtime, "caution-channel", &sample, 0));runtime.screen=screen;
    if(screen==OSG_SCREEN_ERROR)strcpy(runtime.screen_message,"Open panel: Instance already has a writer; an operator must disconnect/take over first.");
    osg_render(&runtime, 100); assert(!raster.failed);
    if (path) {
        FILE *f = fopen(path, "wb"); assert(f); fprintf(f, "P6\n%d %d\n255\n", width, height);
        for (int i = 0; i < width * height; i++) {
            const unsigned char rgb[] = {(unsigned char)(((pixels[i] >> 11) & 31) * 255 / 31), (unsigned char)(((pixels[i] >> 5) & 63) * 255 / 63), (unsigned char)((pixels[i] & 31) * 255 / 31)};
            assert(fwrite(rgb, 1, 3, f) == 3);
        }
        assert(!fclose(f));
    }
    osg_display d = osg_raster_display(&raster, (uint16_t)width, (uint16_t)height);
    d.text(d.context, 0, 0, width, 18, "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .,:;-_+=|/%!()?", 0xffff);
    d.text(d.context, 0, 20, width, 5, "abcdef", 0xffff); d.text(d.context, 0, 0, 1, 1, "", 0xffff);
    fail_write = true; osg_render(&runtime, 100); assert(raster.failed); fail_write = false;
    free(pixels);
}
static uint16_t incremental_pixels[800*480],reference_pixels[800*480];
typedef struct { uint16_t *pixels; int width,height;size_t written;bool fail,limited;int x,y,w,h; } surface;
static bool surface_write(void *ctx,int x,int y,int w,int h,const uint16_t *pixels){
    surface *s=ctx;assert(w>0&&h>0&&w*h<=512&&x>=0&&y>=0&&x+w<=s->width&&y+h<=s->height);
    if(s->limited)assert(x>=s->x&&y>=s->y&&x+w<=s->x+s->w&&y+h<=s->y+s->h);
    if(s->fail)return false;
    for(int row=0;row<h;row++)memcpy(s->pixels+(y+row)*s->width+x,pixels+row*w,(size_t)w*2);
    s->written+=(size_t)w*h;return true;
}
static void compare_render(osg_runtime *r,surface *actual,uint64_t now){
    surface expected={.pixels=reference_pixels,.width=actual->width,.height=actual->height};
    osg_raster reference={.context=&expected,.write=surface_write};
    osg_display original=r->display;r->display=osg_raster_display(&reference,original.width,original.height);
    osg_render(r,now);r->display=original;
    assert(!memcmp(actual->pixels,reference_pixels,(size_t)actual->width*actual->height*2));
}
static void partial_render_test(void){
    for(int size=0;size<2;size++){
        int width=size?800:480,height=size?480:320;
        surface actual={.pixels=incremental_pixels,.width=width,.height=height};
        osg_raster raster={.context=&actual,.write=surface_write};
        osg_runtime r,old;
        assert(osg_init(&r,&osg_demo_panel,osg_raster_display(&raster,width,height),NULL,NULL));
        osg_damage d=osg_render_damage(&r,NULL,0,0);assert(d.full);
        osg_raster_render(&raster,&r,d,0);compare_render(&r,&actual,0);
        old=r;actual.written=0;
        d=osg_render_damage(&r,&old,1,0);assert(!d.full&&!d.widgets);
        osg_raster_render(&raster,&r,d,1);assert(!actual.written);
        r.active=0;d=osg_render_damage(&r,&old,2,1);assert(!d.full&&d.widgets==1);
        osg_widget_rect(&r,0,&actual.x,&actual.y,&actual.w,&actual.h);actual.limited=true;
        osg_raster_render(&raster,&r,d,2);compare_render(&r,&actual,2);
        assert(actual.written<(size_t)width*height/2);
        old=r;r.active=-1;d=osg_render_damage(&r,&old,3,2);
        osg_raster_render(&raster,&r,d,3);compare_render(&r,&actual,3);actual.limited=false;
        old=r;r.samples[1]=(osg_sample){.available=true,.kind=OSG_BOOL,.boolean=true,.received_ms=10};
        d=osg_render_damage(&r,&old,10,3);assert(d.widgets&(1u<<1));
        osg_raster_render(&raster,&r,d,10);compare_render(&r,&actual,10);
        old=r;d=osg_render_damage(&r,&old,1011,10);assert(d.widgets&(1u<<1));
        osg_raster_render(&raster,&r,d,1011);compare_render(&r,&actual,1011);
        old=r;r.connected=true;d=osg_render_damage(&r,&old,1012,1011);assert(d.full);
        osg_raster_render(&raster,&r,d,1012);compare_render(&r,&actual,1012);
        old=r;r.armed=true;assert(osg_render_damage(&r,&old,0,0).full);
        old=r;r.arming=true;assert(osg_render_damage(&r,&old,0,0).full);
        old=r;r.live_input=true;assert(osg_render_damage(&r,&old,0,0).full);
        old=r;r.page=1;assert(osg_render_damage(&r,&old,0,0).full);
        actual.fail=true;osg_raster_render(&raster,&r,(osg_damage){.full=true},1013);assert(raster.failed);
        size_t written=actual.written;osg_raster_render(&raster,&r,(osg_damage){.full=true},1013);assert(actual.written==written);
        actual.fail=false;raster.failed=false;
        osg_raster_render(&raster,&r,(osg_damage){.full=true},1013);compare_render(&r,&actual,1013);
        /* Replaying all layers inside the dirty clip must preserve an overlay. */
        osg_widget widgets[OSG_MAX_WIDGETS];memcpy(widgets,r.panel->widgets,r.panel->widget_count*sizeof(*widgets));
        osg_panel panel=*r.panel;panel.widgets=widgets;widgets[1].bounds=widgets[0].bounds;
        r.panel=&panel;r.page=0;r.active=-1;osg_raster_render(&raster,&r,(osg_damage){.full=true},1014);
        old=r;r.active=0;d=osg_render_damage(&r,&old,1015,1014);
        osg_raster_render(&raster,&r,d,1015);compare_render(&r,&actual,1015);
    }
}
static void image_test(void){
    static const unsigned char pixels[]={0,248,255,224,7,255,31,0,255,255,255,0};
    osg_widget image={.id="image",.reference="",.output_id="",.label="Image",.prefix="",.suffix="",.unit="",.kind=OSG_IMAGE,.bounds={0,0,1,1},.pixels=pixels,.image_width=2,.image_height=2};
    osg_panel panel=osg_demo_panel;panel.widgets=&image;panel.widget_count=1;panel.page_count=1;
    osg_runtime runtime;osg_raster raster={.write=write_pixels};
    assert(osg_init(&runtime,&panel,osg_raster_display(&raster,800,480),NULL,NULL));
    int x,y,w,h;osg_widget_rect(&runtime,0,&x,&y,&w,&h);assert(x==0&&y==0&&w==800&&h==480);
    // Clipped tile composition must sample the source at the correct display position.
    raster.composing=true;raster.tile_x=0;raster.tile_y=0;raster.tile_width=2;raster.tile_height=2;
    osg_render(&runtime,0);assert(raster.tile[0]==0xf800 && raster.tile[3]==0xf800);
    raster.tile_x=799;raster.tile_y=479;raster.tile_width=1;raster.tile_height=1;
    osg_render(&runtime,0);assert(raster.tile[0]==0x08c4); // transparent source preserves canvas
    spy s={.width=800,.height=480};runtime.display=(osg_display){&s,800,480,rect,label,NULL};osg_render(&runtime,0);assert(s.draws>0&&s.texts==0);
    image.pixels=NULL;osg_render(&runtime,0);
}
int main(int argc, char **argv) {
    for(int size=0;size<3;size++){
        spy status;osg_runtime *r=setup(&status,size==0?480:size==1?800:320,size==0?320:size==1?480:240);
        const char *messages[]={"Pair with OSG Cockpit","No panel assigned","No connection to OSG Cockpit","Panel could not render correctly","Connecting to OSG Cockpit","Loading panel","Missing failure diagnostics"};
        for(int screen=OSG_SCREEN_UNPAIRED;screen<=OSG_SCREEN_ERROR;screen++){
            osg_runtime previous=*r;r->screen=(osg_screen_state)screen;
            assert(osg_render_damage(r,&previous,0,0).full);
            status.log[0]=0;status.texts=0;osg_render(r,0);
            assert(status.texts==1 && strstr(status.log,messages[screen-1]));
            const int font=status.text_height/5;
            assert(abs(2*status.text_x+status.text_width-font-status.width)<=1);
            assert(status.text_y>status.height/2 && status.draws>20);
        }
        osg_runtime previous=*r;strcpy(r->screen_message,"Panel image download failed");
        assert(osg_render_damage(r,&previous,0,0).full);
        status.log[0]=0;osg_render(r,0);assert(strstr(status.log,"Panel image download failed"));
        previous=*r;strcpy(r->screen_message,"Open panel: Instance already has a writer; an operator must disconnect/take over first. Stop the active writer before reconnecting this companion.");
        assert(osg_render_damage(r,&previous,0,0).full);
        status.log[0]=0;status.texts=0;osg_render(r,0);assert(status.texts>1);
        assert(strstr(status.log,"Open panel:")&&strstr(status.log,"first."));
        r->screen_message[0]=0;
        r->screen=OSG_SCREEN_PANEL;status.log[0]=0;osg_render(r,0);
        assert(!strstr(status.log,"LIVE INPUT")&&!strstr(status.log,"RECORDING ONLY"));
    }
    image_test();partial_render_test();
    validation(); input(); feedback(); bridge_test();
    for (int i = 0; i < 3; i++) { spy s; osg_runtime *r = setup(&s, i == 0 ? 800 : i == 1 ? 320 : 480, i == 0 ? 480 : i == 1 ? 240 : 800); osg_render(r, 0); assert(s.draws && s.texts); }
    preview(argc > 1 ? argv[1] : NULL, 800, 480,OSG_SCREEN_PANEL); preview(NULL, 320, 240,OSG_SCREEN_PANEL); preview(NULL, 480, 800,OSG_SCREEN_PANEL);
    for(int screen=OSG_SCREEN_UNPAIRED;screen<=OSG_SCREEN_ERROR;screen++){
        char path[512];if(argc>2){snprintf(path,sizeof(path),"%s/status-%d.ppm",argv[2],screen);}
        preview(argc>2?path:NULL,480,320,(osg_screen_state)screen);
        preview(NULL,800,480,(osg_screen_state)screen);
    }
    printf("All generic runtime, recording input, feedback and RGB565 raster checks passed. Runtime=%zu bytes; raster=%zu bytes\n", sizeof(osg_runtime), sizeof(osg_raster));
    return 0;
}
