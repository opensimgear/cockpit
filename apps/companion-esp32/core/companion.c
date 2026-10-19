#include "companion.h"
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <limits.h>

static bool text_valid(const char *s, size_t max, bool empty) {
    if (!s) return false;
    size_t n = 0;
    while (s[n]) {
        if (n >= max || (unsigned char)s[n] < 32 || (unsigned char)s[n] > 126) return false;
        const unsigned char c = (unsigned char)s[n];
        if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
              (c >= '0' && c <= '9') || strchr(" .,:;-_+=|/%!()?", c))) return false;
        n++;
    }
    return empty || n > 0;
}
static bool id_valid(const char *s) {
    if (!text_valid(s, 64, false)) return false;
    for (size_t i = 0; s[i]; i++) {
        const char c = s[i];
        if (!((c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') ||
              (c >= '0' && c <= '9') || (i && (c == '_' || c == '-')))) return false;
    }
    return true;
}
static bool hash_valid(const char *s) {
    if (!s || strlen(s) != 71 || strncmp(s, "sha256:", 7)) return false;
    for (size_t i = 7; i < 71; i++)
        if (!((s[i] >= '0' && s[i] <= '9') || (s[i] >= 'a' && s[i] <= 'f'))) return false;
    return true;
}
bool osg_panel_valid(const osg_panel *p) {
    if (!p || !id_valid(p->ui_id) || !hash_valid(p->publication_hash) || !p->pages || !p->widgets ||
        !p->page_count || p->page_count > OSG_MAX_PAGES || !p->widget_count || p->widget_count > OSG_MAX_WIDGETS) return false;
    for (size_t i = 0; i < p->page_count; i++) {
        const osg_page *page = &p->pages[i];
        if (!id_valid(page->id) || !text_valid(page->title, 120, false) || page->width < 32 || page->height < 32 ||
            page->width > 8192 || page->height > 8192) return false;
        for (size_t j = 0; j < i; j++) if (!strcmp(page->id, p->pages[j].id)) return false;
    }
    for (size_t i = 0; i < p->widget_count; i++) {
        const osg_widget *w = &p->widgets[i];
        const osg_bounds b = w->bounds;
        if (!id_valid(w->id) || !id_valid(w->reference) || !text_valid(w->label, 120, false) ||
            !text_valid(w->prefix, 120, true) || !text_valid(w->suffix, 120, true) || !text_valid(w->unit, 32, true) ||
            w->page >= p->page_count || w->kind < OSG_BUTTON || w->kind > OSG_GAUGE || w->precision > 4 ||
            !isfinite(b.x) || !isfinite(b.y) || !isfinite(b.width) || !isfinite(b.height) ||
            b.x < 0 || b.y < 0 || b.width <= 0 || b.height <= 0 || b.x + b.width > 1.000000001 || b.y + b.height > 1.000000001) return false;
        if (w->kind != OSG_BUTTON && !id_valid(w->output_id)) return false;
        if ((w->kind == OSG_GAUGE || w->kind == OSG_NUMERIC) &&
            (!isfinite(w->min) || !isfinite(w->max) || w->min >= w->max || w->min < -1e9 || w->max > 1e9)) return false;
        for (size_t j = 0; j < i; j++) {
            const osg_widget *v = &p->widgets[j];
            if (!strcmp(w->id, v->id) || (w->kind == OSG_BUTTON && v->kind == OSG_BUTTON && !strcmp(w->reference, v->reference))) return false;
            /* Initial single-pointer subset rejects overlapping touch targets. */
            const osg_bounds a = v->bounds;
            if (w->page == v->page && (w->kind == OSG_BUTTON || v->kind == OSG_BUTTON) &&
                b.x < a.x + a.width && a.x < b.x + b.width && b.y < a.y + a.height && a.y < b.y + b.height) return false;
        }
    }
    return true;
}
bool osg_init(osg_runtime *r, const osg_panel *p, osg_display d, osg_input_sink sink, void *ctx) {
    if (!r || !osg_panel_valid(p) || !d.fill || !d.text || d.width < 64 || d.height < 64 || d.width > 4096 || d.height > 4096) return false;
    memset(r, 0, sizeof(*r));
    r->panel = p; r->display = d; r->active = -1; r->sink = sink; r->sink_context = ctx;
    r->await_release = true;
    return true;
}
static bool edge(osg_runtime *r, size_t index, bool down) {
    if (!r->sink || r->sequence == UINT64_MAX || !r->sink(r->sink_context, r->panel->widgets[index].reference, down, r->sequence + 1)) return false;
    r->sequence++;
    return true;
}
void osg_disarm(osg_runtime *r) {
    const int active = r->active;
    r->active = -1; r->armed = false; r->live_input = false; r->arming = false; r->await_release = true;
    if (active >= 0) (void)edge(r, (size_t)active, false);
}
bool osg_preview_arm(osg_runtime *r) {
    if (r->pointer_down || r->await_release || !r->sink || r->sequence == UINT64_MAX) return false;
    r->armed = true; r->live_input = false;
    return true;
}
bool osg_live_arm(osg_runtime *r) {
    if (!r->connected || !osg_preview_arm(r)) return false;
    r->live_input = true;
    return true;
}
bool osg_set_page(osg_runtime *r, size_t page) {
    if (page >= r->panel->page_count) return false;
    osg_disarm(r); r->page = (uint8_t)page;
    return true;
}
static void geometry(const osg_runtime *r, const osg_widget *w, int *x, int *y, int *width, int *height) {
    const osg_page *p = &r->panel->pages[r->page];
    double scale = fmin((double)r->display.width / p->width, (double)(r->display.height - 32) / p->height);
    const double pw = p->width * scale, ph = p->height * scale;
    *x = (int)floor((r->display.width - pw) / 2 + w->bounds.x * pw);
    *y = 24 + (int)floor((r->display.height - 32 - ph) / 2 + w->bounds.y * ph);
    *width = (int)floor(w->bounds.width * pw); *height = (int)floor(w->bounds.height * ph);
}
void osg_touch(osg_runtime *r, bool down, int x, int y) {
    const bool was_down = r->pointer_down;
    r->pointer_down = down;
    if (!down) {
        const int active = r->active; r->active = -1; r->await_release = false;
        if (active >= 0 && !edge(r, (size_t)active, false)) osg_disarm(r);
        return;
    }
    if (!r->armed || r->await_release) return;
    if (r->active >= 0) {
        int bx, by, bw, bh; geometry(r, &r->panel->widgets[r->active], &bx, &by, &bw, &bh);
        if (x < bx || y < by || x >= bx + bw || y >= by + bh) osg_disarm(r);
        return;
    }
    if (was_down) return;
    /* Reverse paint order matches the visible topmost target. */
    for (size_t i = r->panel->widget_count; i > 0; i--) {
        const osg_widget *w = &r->panel->widgets[i - 1];
        if (w->page != r->page || w->kind != OSG_BUTTON) continue;
        int bx, by, bw, bh; geometry(r, w, &bx, &by, &bw, &bh);
        if (bw < 44 || bh < 44 || x < bx || y < by || x >= bx + bw || y >= by + bh) continue;
        if (edge(r, i - 1, true)) r->active = (int)i - 1;
        else osg_disarm(r);
        return;
    }
}
void osg_connected(osg_runtime *r, bool connected) {
    /* New connection state never grants input authority. */
    if (connected != r->connected) { osg_disarm(r); memset(r->samples, 0, sizeof(r->samples)); }
    r->connected = connected;
}
bool osg_feedback(osg_runtime *r, const char *channel, const osg_sample *s, uint64_t now) {
    if (!r->connected || !channel || !s || s->provenance < OSG_REQUESTED || s->provenance > OSG_GAME_CONFIRMED ||
        s->source_age_ms > 86400000 || !s->available) return false;
    /* Validate every alias before committing any sample. */
    bool found = false;
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        const osg_widget *w = &r->panel->widgets[i];
        if (w->kind == OSG_BUTTON || strcmp(w->reference, channel)) continue;
        found = true;
        if (w->kind == OSG_TEXT) {
            if (s->kind != OSG_STRING || !memchr(s->text, 0, sizeof(s->text)) || !text_valid(s->text, 120, true)) return false;
        } else if (w->kind == OSG_INDICATOR) { if (s->kind != OSG_BOOL) return false; }
        else if (s->kind != OSG_NUMBER || !isfinite(s->number) || s->number < w->min || s->number > w->max) return false;
    }
    if (!found) return false;
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        const osg_widget *w = &r->panel->widgets[i];
        if (w->kind != OSG_BUTTON && !strcmp(w->reference, channel)) { r->samples[i] = *s; r->samples[i].received_ms = now; }
    }
    return true;
}
static void fill(osg_runtime *r, int x, int y, int w, int h, uint16_t color) {
    if (x < 0) { w += x; x = 0; } if (y < 0) { h += y; y = 0; }
    if (x + w > r->display.width) w = r->display.width - x;
    if (y + h > r->display.height) h = r->display.height - y;
    if (w > 0 && h > 0) r->display.fill(r->display.context, x, y, w, h, color);
}
static void text(osg_runtime *r, int x, int y, int w, int h, const char *s, uint16_t color) {
    if (w <= 0 || h <= 0 || x < 0 || y < 0 || x >= r->display.width || y >= r->display.height) return;
    if (x + w > r->display.width) w = r->display.width - x;
    if (y + h > r->display.height) h = r->display.height - y;
    r->display.text(r->display.context, x, y, w, h, s, color);
}
void osg_widget_rect(const osg_runtime *r,size_t index,int *x,int *y,int *width,int *height){
    geometry(r,&r->panel->widgets[index],x,y,width,height);
}
/* Status icons use the same bounded rectangle backend as widgets, without font
 * glyph dependencies or image assets. Coordinates are on a 24-unit grid. */
static void status_line(osg_runtime *r,int x,int y,int unit,int x0,int y0,int x1,int y1,uint16_t color){
    const int dx=x1>x0?x1-x0:x0-x1,dy=y1>y0?y0-y1:y1-y0;
    const int sx=x0<x1?1:-1,sy=y0<y1?1:-1;int error=dx+dy;
    for(;;){
        fill(r,x+x0*unit,y+y0*unit,unit,unit,color);
        if(x0==x1&&y0==y1)break;
        const int twice=2*error;if(twice>=dy){error+=dy;x0+=sx;}if(twice<=dx){error+=dx;y0+=sy;}
    }
}
static void status_screen(osg_runtime *r){
    const char *message=r->screen==OSG_SCREEN_UNPAIRED?"Pair with OSG Cockpit":
        r->screen==OSG_SCREEN_NO_PANEL?"No panel assigned":
        r->screen==OSG_SCREEN_PANEL_ERROR?"Panel could not render correctly":"No connection to OSG Cockpit";
    int unit=r->display.height/100;if(unit<1)unit=1;if(unit>4)unit=4;
    int font=3;const int length=(int)strlen(message);
    while(font>1&&length*4*font>r->display.width-32)font--;
    const int icon=24*unit,gap=5*unit,height=icon+gap+5*font;
    const int x=(r->display.width-25*unit)/2,y=(r->display.height-height)/2;
    const uint16_t color=r->screen==OSG_SCREEN_PANEL_ERROR?0xfbaa:0x8d18;
#define LINE(a,b,c,d) status_line(r,x,y,unit,a,b,c,d,color)
    if(r->screen==OSG_SCREEN_DISCONNECTED){
        /* Two unplugged connectors, with a break between their exposed pins. */
        LINE(1,12,4,12);LINE(4,7,4,17);LINE(4,7,9,7);LINE(4,17,9,17);LINE(9,7,9,17);
        LINE(9,9,11,9);LINE(9,15,11,15);
        LINE(23,12,20,12);LINE(20,7,20,17);LINE(20,7,15,7);LINE(20,17,15,17);LINE(15,7,15,17);
        LINE(13,9,15,9);LINE(13,15,15,15);LINE(14,2,10,22);
    }else if(r->screen==OSG_SCREEN_PANEL_ERROR){
        LINE(8,2,16,2);LINE(16,2,22,8);LINE(22,8,22,16);LINE(22,16,16,22);
        LINE(16,22,8,22);LINE(8,22,2,16);LINE(2,16,2,8);LINE(2,8,8,2);
        LINE(12,7,12,13);LINE(12,17,12,18);
    }else{
        LINE(3,3,21,3);LINE(21,3,21,21);LINE(21,21,3,21);LINE(3,21,3,3);LINE(3,7,21,7);
        if(r->screen==OSG_SCREEN_NO_PANEL){LINE(1,23,23,1);}
        else{LINE(12,10,12,18);LINE(8,14,16,14);}
    }
#undef LINE
    const int width=length*4*font,ink=width-font;
    text(r,(r->display.width-ink)/2,y+icon+gap,width,5*font,message,0xdedc);
}
static bool render_stale(const osg_sample *s,uint64_t now){
    return s->source_status==OSG_FEEDBACK_STALE || (s->available &&
        (now<s->received_ms || s->source_age_ms>=OSG_STALE_MS ||
         now-s->received_ms>=OSG_STALE_MS-s->source_age_ms));
}
osg_damage osg_render_damage(const osg_runtime *r,const osg_runtime *old,uint64_t now,uint64_t before){
    osg_damage d={0};
    if(!old || !old->panel || r->panel!=old->panel || r->page!=old->page ||
       r->display.width!=old->display.width || r->display.height!=old->display.height ||
       r->screen!=old->screen || r->connected!=old->connected || r->armed!=old->armed ||
       r->arming!=old->arming || r->live_input!=old->live_input){d.full=true;return d;}
    for(size_t i=0;i<r->panel->widget_count;i++){
        const osg_widget *w=&r->panel->widgets[i];
        if(w->page!=r->page)continue;
        bool changed=w->kind==OSG_BUTTON
            ? ((r->active==(int)i)!=(old->active==(int)i))
            : (memcmp(&r->samples[i],&old->samples[i],sizeof(osg_sample))!=0 ||
               render_stale(&r->samples[i],now)!=render_stale(&old->samples[i],before));
        if(changed)d.widgets|=UINT32_C(1)<<i;
    }
    return d;
}
void osg_render(osg_runtime *r, uint64_t now) {
    static const char *provenance[] = {"REQUESTED", "LOCAL", "HOST ACCEPTED", "HOST SUBMITTED", "GAME CONFIRMED"};
    fill(r, 0, 0, r->display.width, r->display.height, 0x08c4);
    if(r->screen!=OSG_SCREEN_PANEL){
        status_screen(r);
        return;
    }
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        const osg_widget *w = &r->panel->widgets[i]; if (w->page != r->page) continue;
        int x, y, width, height; geometry(r, w, &x, &y, &width, &height);
        fill(r, x, y, width, height, w->background);
        text(r, x + 4, y + 3, width - 8, height < 20 ? height - 6 : 17, w->label, w->foreground);
        if (w->kind == OSG_BUTTON) {
            fill(r, x + 4, y + 24, width - 8, height - 28, r->active == (int)i ? w->accent : 0x2128);
            text(r, x + 8, y + 28, width - 16, height - 32, r->armed ? "PRESS" : "DISARMED", w->foreground);
            continue;
        }
        const osg_sample *s = &r->samples[i];
        const bool known = s->available && r->connected && s->source_status != OSG_FEEDBACK_UNKNOWN && s->source_status != OSG_FEEDBACK_DISCONNECTED;
        const bool stale = s->source_status == OSG_FEEDBACK_STALE || (known && (now < s->received_ms || now - s->received_ms >= OSG_STALE_MS ||
            s->source_age_ms >= OSG_STALE_MS || now - s->received_ms >= OSG_STALE_MS - s->source_age_ms));
        char value[384], status[64];
        if (!known) (void)snprintf(value, sizeof(value), "%s", !r->connected || s->source_status == OSG_FEEDBACK_DISCONNECTED ? "DISCONNECTED" : stale ? "STALE" : "UNKNOWN");
        else if (w->kind == OSG_TEXT) (void)snprintf(value, sizeof(value), "%s%s%s", w->prefix, s->text, w->suffix);
        else if (w->kind == OSG_INDICATOR) (void)snprintf(value, sizeof(value), "%s", s->boolean ? "ON" : "OFF");
        else (void)snprintf(value, sizeof(value), "%.*f %s", w->precision, s->number, w->unit);
        text(r, x + 4, y + 24, width - 8, height - 44, value, stale ? 0xfd20 : w->foreground);
        if (w->kind == OSG_GAUGE && known && height >= 66) {
            const int bw = width - 8;
            fill(r, x + 4, y + height - 28, bw, 6, 0x2128);
            fill(r, x + 4, y + height - 28, (int)(bw * (s->number - w->min) / (w->max - w->min)), 6, stale ? 0xfd20 : w->accent);
        }
        (void)snprintf(status, sizeof(status), "%s%s", known ? provenance[s->provenance] : "UNKNOWN", known ? stale ? " | STALE" : " | FRESH" : "");
        text(r, x + 4, y + height - 17, width - 8, 14, status, stale ? 0xfd20 : 0x95b3);
    }
}
