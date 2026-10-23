#include "companion.h"
#include "widget.h"
#include "../render/widget_state.h"
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
    return osg_widget_id_valid(s);
}
static bool hash_valid(const char *s) {
    if (!s || strlen(s) != 71 || strncmp(s, "sha256:", 7)) return false;
    for (size_t i = 7; i < 71; i++)
        if (!((s[i] >= '0' && s[i] <= '9') || (s[i] >= 'a' && s[i] <= 'f'))) return false;
    return true;
}
static bool interactive(const osg_widget *w) {
    return w->kind == OSG_BUTTON || (w->kind == OSG_COMPONENT && w->component && w->component->definition && w->component->definition->interaction != OSG_INTERACTION_NONE);
}
osg_bounds osg_widget_artwork_bounds(const osg_widget *w, const osg_page *page) {
    osg_bounds b = w->bounds;
    if (w->kind == OSG_COMPONENT && w->component && w->component->definition && w->component->definition->width && w->component->definition->height) {
        const osg_widget_component *d = w->component->definition;
        const double scale = fmin(b.width * page->width / d->width, b.height * page->height / d->height);
        const double width = d->width * scale / page->width, height = d->height * scale / page->height;
        b.x += (b.width - width) / 2; b.y += (b.height - height) / 2;
        b.width = width; b.height = height;
    }
    return b;
}
static osg_bounds hit_bounds(const osg_panel *p, const osg_widget *w) {
    osg_bounds b = osg_widget_artwork_bounds(w, &p->pages[w->page]);
    if (w->kind == OSG_COMPONENT) {
        const osg_bounds h = w->component->definition->hit_area;
        b.x += b.width * h.x; b.y += b.height * h.y; b.width *= h.width; b.height *= h.height;
    }
    return b;
}
bool osg_panel_valid(const osg_panel *p) {
    if (!p || !id_valid(p->ui_id) || !hash_valid(p->publication_hash) || !p->pages || !p->widgets ||
        !p->page_count || p->page_count > OSG_MAX_PAGES || !p->widget_count || p->widget_count > OSG_MAX_WIDGETS) return false;
    for (size_t i = 0; i < p->page_count; i++) {
        const osg_page *page = &p->pages[i];
        if (!id_valid(page->id) || !osg_widget_text_valid(page->title) || !*page->title || page->width < 32 || page->height < 32 ||
            page->width > 8192 || page->height > 8192) return false;
        for (size_t j = 0; j < i; j++) if (!strcmp(page->id, p->pages[j].id)) return false;
    }
    bool output_slots[OSG_MAX_WIDGETS] = {false}; const char *output_ids[OSG_MAX_WIDGETS]; size_t output_count = 0;
    for (size_t i = 0; i < p->widget_count; i++) {
        const osg_widget *w = &p->widgets[i];
        const osg_bounds b = w->bounds;
        if (!id_valid(w->id) || (w->kind != OSG_IMAGE && w->kind != OSG_COMPONENT && !id_valid(w->reference)) ||
            !(w->kind == OSG_COMPONENT ? osg_widget_text_valid(w->label) : text_valid(w->label, 120, false)) ||
            !text_valid(w->prefix, 120, true) || !text_valid(w->suffix, 120, true) || !text_valid(w->unit, 32, true) ||
            w->page >= p->page_count || w->kind < OSG_BUTTON || w->kind > OSG_COMPONENT || w->precision > 4 ||
            !isfinite(b.x) || !isfinite(b.y) || !isfinite(b.width) || !isfinite(b.height) ||
            b.x < 0 || b.y < 0 || b.width <= 0 || b.height <= 0 || b.x + b.width > 1.000000001 || b.y + b.height > 1.000000001) return false;
        if (w->kind == OSG_COMPONENT) {
            const osg_widget_binding *binding = w->component;
            if (!binding || !binding->definition) return false;
            const osg_widget_component *d = binding->definition;
            if (!id_valid(d->id) || !d->version || !d->layers || !d->layer_count || d->layer_count > 32 || d->output_count > OSG_WIDGET_SOURCE_PORTS || d->interaction < OSG_INTERACTION_NONE || d->interaction > OSG_INTERACTION_NAVIGATE) return false;
            if (d->interaction == OSG_INTERACTION_SELECTOR && (d->option_count < 2 || d->option_count > 16 || !isfinite(d->fallback))) return false;
            for (unsigned port = 0; port < d->output_count; port++) {
                const osg_bound_output *out = &binding->outputs[port];
                if (!id_valid(out->id) || !id_valid(out->channel) || out->slot >= OSG_MAX_WIDGETS || output_slots[out->slot] || output_count >= OSG_MAX_WIDGETS) return false;
                for (size_t before = 0; before < output_count; before++) if (!strcmp(output_ids[before], out->id)) return false;
                output_slots[out->slot] = true; output_ids[output_count++] = out->id;
            }
        } else if (w->kind != OSG_BUTTON && w->kind != OSG_IMAGE) {
            if (!id_valid(w->output_id) || output_slots[i]) return false;
            for (size_t before = 0; before < output_count; before++) if (!strcmp(output_ids[before], w->output_id)) return false;
            output_slots[i] = true; output_ids[output_count++] = w->output_id;
        }
        osg_value_kind input_kind; double fallback;
        const bool input = osg_widget_input(w, &input_kind, &fallback);
        if (input && !id_valid(w->reference)) return false;
        if ((w->kind == OSG_GAUGE || w->kind == OSG_NUMERIC) &&
            (!isfinite(w->min) || !isfinite(w->max) || w->min >= w->max || w->min < -1e9 || w->max > 1e9)) return false;
        for (size_t j = 0; j < i; j++) {
            const osg_widget *v = &p->widgets[j];
            osg_value_kind before_kind; double before_fallback;
            if (!strcmp(w->id, v->id) || (input && osg_widget_input(v, &before_kind, &before_fallback) && !strcmp(w->reference, v->reference))) return false;
            /* Initial single-pointer subset rejects overlapping touch targets. */
            const bool portable = w->kind == OSG_COMPONENT || v->kind == OSG_COMPONENT;
            const osg_bounds a = portable ? hit_bounds(p, v) : v->bounds, touch = portable ? hit_bounds(p, w) : b;
            if (w->page == v->page && (portable ? interactive(w) && interactive(v) : w->kind != OSG_IMAGE && v->kind != OSG_IMAGE && (w->kind == OSG_BUTTON || v->kind == OSG_BUTTON)) &&
                touch.x < a.x + a.width && a.x < touch.x + touch.width && touch.y < a.y + a.height && a.y < touch.y + touch.height) return false;
        }
    }
    if (p->metadata) {
        const osg_panel_metadata *m = p->metadata;
        if (m->group_count > 64 || (m->group_count && !m->groups)) return false;
        for (size_t i = 0; i < m->group_count; i++) {
            const osg_widget_group *group = &m->groups[i];
            if (!id_valid(group->id) || !osg_widget_text_valid(group->label) || osg_widget_text_blank(group->label) || group->page >= p->page_count || (p->widget_count < 32 && group->members >> p->widget_count)) return false;
            for (size_t before = 0; before < i; before++) if (!strcmp(group->id, m->groups[before].id)) return false;
            for (size_t w = 0; w < p->widget_count; w++) if ((group->members & (UINT32_C(1) << w)) && p->widgets[w].page != group->page) return false;
        }
    }
    return true;
}
bool osg_init(osg_runtime *r, const osg_panel *p, osg_display d, osg_input_sink sink, void *ctx) {
    if (!r || !osg_panel_valid(p) || !d.fill || !d.text || d.width < 64 || d.height < 64 || d.width > 4096 || d.height > 4096) return false;
    memset(r, 0, sizeof(*r));
    r->panel = p; r->display = d; r->active = -1; r->sink = sink; r->sink_context = ctx;
    r->await_release = true;
    for (size_t i = 0; i < p->widget_count; i++) { osg_value_kind kind; double fallback; if (osg_widget_input(&p->widgets[i], &kind, &fallback)) r->requested[i] = fallback; }
    return true;
}
static bool edge(osg_runtime *r, size_t index, double value) {
    osg_value_kind kind; double fallback;
    if (!osg_widget_input(&r->panel->widgets[index], &kind, &fallback) || r->sequence == UINT64_MAX) return false;
    const bool accepted = r->typed_sink ? r->typed_sink(r->sink_context, r->panel->widgets[index].reference, kind, value, r->sequence + 1) : kind == OSG_BOOL && r->sink && r->sink(r->sink_context, r->panel->widgets[index].reference, value != 0, r->sequence + 1);
    if (!accepted) return false;
    r->sequence++;
    return true;
}
void osg_disarm(osg_runtime *r) {
    r->active = -1; r->armed = false; r->live_input = false; r->arming = false; r->await_release = true;
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        osg_value_kind kind; double fallback;
        if (!osg_widget_input(&r->panel->widgets[i], &kind, &fallback) || r->requested[i] == fallback) continue;
        r->requested[i] = fallback; (void)edge(r, i, fallback);
    }
}
bool osg_preview_arm(osg_runtime *r) {
    if (r->pointer_down || r->await_release || (!r->sink && !r->typed_sink) || r->sequence == UINT64_MAX) return false;
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
static void panel_geometry(const osg_panel *panel, const osg_widget *w, osg_bounds bounds, uint16_t display_width, uint16_t display_height, int *x, int *y, int *width, int *height) {
    const osg_page *p = &panel->pages[w->page];
    double scale = fmin((double)display_width / p->width, (double)display_height / p->height);
    const double pw = p->width * scale, ph = p->height * scale;
    *x = (int)floor((display_width - pw) / 2 + bounds.x * pw);
    *y = (int)floor((display_height - ph) / 2 + bounds.y * ph);
    *width = (int)floor(bounds.width * pw); *height = (int)floor(bounds.height * ph);
}
static void geometry_bounds(const osg_runtime *r, const osg_widget *w, osg_bounds bounds, int *x, int *y, int *width, int *height) { panel_geometry(r->panel, w, bounds, r->display.width, r->display.height, x, y, width, height); }
void osg_panel_widget_rect(const osg_panel *p, size_t i, uint16_t dw, uint16_t dh, int *x, int *y, int *width, int *height) { panel_geometry(p, &p->widgets[i], osg_widget_artwork_bounds(&p->widgets[i], &p->pages[p->widgets[i].page]), dw, dh, x, y, width, height); }
static void geometry(const osg_runtime *r, const osg_widget *w, int *x, int *y, int *width, int *height) { geometry_bounds(r, w, osg_widget_artwork_bounds(w, &r->panel->pages[w->page]), x, y, width, height); }
static bool change(osg_runtime *r, size_t index, double value) {
    if (r->requested[index] == value) return true;
    if (!edge(r, index, value)) { osg_disarm(r); return false; }
    r->requested[index] = value; return true;
}
static void cancel_widget(osg_runtime *r, size_t index) {
    if (r->active == (int)index) { r->active = -1; r->await_release = true; }
    osg_value_kind kind; double fallback;
    if (osg_widget_input(&r->panel->widgets[index], &kind, &fallback)) (void)change(r, index, fallback);
}
void osg_tick(osg_runtime *r, uint64_t now) {
    r->now_ms = now;
    for (size_t i = 0; i < r->panel->widget_count; i++) {
        const osg_widget *w = &r->panel->widgets[i];
        if (w->kind != OSG_COMPONENT || (!r->armed && r->active != (int)i)) continue;
        osg_widget_state_context context; osg_widget_runtime_context(r, i, now, &context);
        if (!osg_widget_gate_open(w->component->definition, &context)) cancel_widget(r, i);
    }
}
void osg_touch(osg_runtime *r, bool down, int x, int y) {
    const bool was_down = r->pointer_down;
    r->pointer_down = down;
    if (!down) {
        const int active = r->active; r->active = -1; r->await_release = false;
        if (active >= 0) {
            const osg_widget *w = &r->panel->widgets[active];
            const osg_widget_component *d = w->kind == OSG_COMPONENT ? w->component->definition : NULL;
            if (d && d->interaction == OSG_INTERACTION_NAVIGATE) {
                for (size_t p = 0; p < r->panel->page_count; p++) if (d->navigate_page && !strcmp(r->panel->pages[p].id, d->navigate_page)) { (void)osg_set_page(r, p); break; }
            } else if (!d || d->interaction == OSG_INTERACTION_MOMENTARY) (void)change(r, (size_t)active, 0);
        }
        return;
    }
    if (r->await_release) return;
    if (r->active >= 0) {
        const osg_widget *w = &r->panel->widgets[r->active];
        int bx, by, bw, bh; geometry_bounds(r, w, hit_bounds(r->panel, w), &bx, &by, &bw, &bh);
        if (x < bx || y < by || x >= bx + bw || y >= by + bh) { if (w->kind == OSG_COMPONENT) cancel_widget(r, (size_t)r->active); else osg_disarm(r); }
        return;
    }
    if (was_down) return;
    /* Reverse paint order matches the visible topmost target. */
    for (size_t i = r->panel->widget_count; i > 0; i--) {
        const osg_widget *w = &r->panel->widgets[i - 1];
        if (w->page != r->page || !interactive(w)) continue;
        const osg_widget_component *d = w->kind == OSG_COMPONENT ? w->component->definition : NULL;
        if (!r->armed && (!d || d->interaction != OSG_INTERACTION_NAVIGATE || r->screen != OSG_SCREEN_PANEL)) continue;
        int bx, by, bw, bh; geometry_bounds(r, w, hit_bounds(r->panel, w), &bx, &by, &bw, &bh);
        if (bw < 44 || bh < 44 || x < bx || y < by || x >= bx + bw || y >= by + bh) continue;
        if (d) {
            osg_widget_state_context context; osg_widget_runtime_context(r, i - 1, r->now_ms, &context);
            if (!osg_widget_gate_open(d, &context)) return;
        }
        double value = 1;
        if (d && d->interaction == OSG_INTERACTION_TOGGLE) value = !r->requested[i - 1];
        if (d && d->interaction == OSG_INTERACTION_SELECTOR) {
            unsigned option = 0;
            while (option < d->option_count && d->options[option] != r->requested[i - 1]) option++;
            value = d->options[(option + 1) % d->option_count];
        }
        if ((d && d->interaction == OSG_INTERACTION_NAVIGATE) || change(r, i - 1, value)) r->active = (int)i - 1;
        return;
    }
}
void osg_connected(osg_runtime *r, bool connected) {
    /* New connection state never grants input authority. */
    if (connected != r->connected) { osg_disarm(r); memset(r->samples, 0, sizeof(r->samples)); }
    r->connected = connected;
}
bool osg_feedback_valid(const osg_panel *panel, const char *channel, const osg_sample *s) {
    if (!panel || !channel || !s || s->provenance < OSG_REQUESTED || s->provenance > OSG_GAME_CONFIRMED ||
        s->source_age_ms > 86400000 || !s->available) return false;
    /* Validate every alias before committing any sample. */
    bool found = false;
    for (size_t i = 0; i < OSG_MAX_WIDGETS; i++) {
        osg_output_contract output;
        if (!osg_panel_output(panel, i, &output) || strcmp(output.channel, channel)) continue;
        found = true;
        if (s->kind != output.kind) return false;
        if (s->kind == OSG_STRING) {
            if (!memchr(s->text, 0, sizeof(s->text)) || !(output.portable_text ? osg_widget_text_valid(s->text) : text_valid(s->text, 120, true))) return false;
        } else if (s->kind == OSG_NUMBER && (!isfinite(s->number) || (output.has_range && (s->number < output.min || s->number > output.max)))) return false;
    }
    return found;
}
bool osg_feedback(osg_runtime *r, const char *channel, const osg_sample *s, uint64_t now) {
    if (!r->connected || !osg_feedback_valid(r->panel, channel, s)) return false;
    for (size_t i = 0; i < OSG_MAX_WIDGETS; i++) {
        osg_output_contract output;
        if (osg_panel_output(r->panel, i, &output) && !strcmp(output.channel, channel)) { r->samples[i] = *s; r->samples[i].received_ms = now; r->samples[i].received = true; }
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
static const char *status_next_line(const char *message,size_t columns,size_t *length){
    size_t count=strlen(message);if(count>columns)count=columns;
    if(message[count]&&message[count]!=' '){
        size_t word=count;while(word&&message[word]!=' ')word--;
        if(word)count=word;
    }
    *length=count;message+=count;while(*message==' ')message++;
    return message;
}
static void status_screen(osg_runtime *r){
    const char *message=r->screen_message[0]?r->screen_message:r->screen==OSG_SCREEN_UNPAIRED?"Pair with OSG Cockpit":
        r->screen==OSG_SCREEN_NO_PANEL?"No panel assigned":
        r->screen==OSG_SCREEN_PANEL_ERROR?"Panel could not render correctly":
        r->screen==OSG_SCREEN_CONNECTING?"Connecting to OSG Cockpit":
        r->screen==OSG_SCREEN_LOADING?"Loading panel":
        r->screen==OSG_SCREEN_ERROR?"Missing failure diagnostics":"No connection to OSG Cockpit";
    int unit=r->display.height/100;if(unit<1)unit=1;if(unit>4)unit=4;
    int font=3;const int length=(int)strlen(message);
    while(font>2&&length*4*font>r->display.width-32)font--;
    const size_t columns=(size_t)((r->display.width-32)/(4*font)>0?(r->display.width-32)/(4*font):1);
    size_t lines=0,count;const char *part=message;
    do{part=status_next_line(part,columns,&count);lines++;}while(*part);
    const int line_height=5*font+2*unit;
    const int icon=24*unit,gap=5*unit,height=icon+gap+(int)lines*line_height-2*unit;
    const int x=(r->display.width-25*unit)/2,y=(r->display.height-height)/2;
    const uint16_t color=(r->screen==OSG_SCREEN_PANEL_ERROR||r->screen==OSG_SCREEN_ERROR)?0xfbaa:0x8d18;
#define LINE(a,b,c,d) status_line(r,x,y,unit,a,b,c,d,color)
    if(r->screen==OSG_SCREEN_DISCONNECTED){
        /* Two unplugged connectors, with a break between their exposed pins. */
        LINE(1,12,4,12);LINE(4,7,4,17);LINE(4,7,9,7);LINE(4,17,9,17);LINE(9,7,9,17);
        LINE(9,9,11,9);LINE(9,15,11,15);
        LINE(23,12,20,12);LINE(20,7,20,17);LINE(20,7,15,7);LINE(20,17,15,17);LINE(15,7,15,17);
        LINE(13,9,15,9);LINE(13,15,15,15);LINE(14,2,10,22);
    }else if(r->screen==OSG_SCREEN_PANEL_ERROR||r->screen==OSG_SCREEN_ERROR){
        LINE(8,2,16,2);LINE(16,2,22,8);LINE(22,8,22,16);LINE(22,16,16,22);
        LINE(16,22,8,22);LINE(8,22,2,16);LINE(2,16,2,8);LINE(2,8,8,2);
        LINE(12,7,12,13);LINE(12,17,12,18);
    }else if(r->screen==OSG_SCREEN_CONNECTING||r->screen==OSG_SCREEN_LOADING){
        /* Hourglass: work is in progress, rather than a disconnected cable. */
        LINE(5,2,19,2);LINE(5,22,19,22);LINE(6,3,6,7);LINE(18,3,18,7);
        LINE(6,7,18,17);LINE(18,7,6,17);LINE(6,17,6,21);LINE(18,17,18,21);
    }else{
        LINE(3,3,21,3);LINE(21,3,21,21);LINE(21,21,3,21);LINE(3,21,3,3);LINE(3,7,21,7);
        if(r->screen==OSG_SCREEN_NO_PANEL){LINE(1,23,23,1);}
        else{LINE(12,10,12,18);LINE(8,14,16,14);}
    }
#undef LINE
    part=message;
    for(size_t row=0;row<lines;row++){
        char line[192];const char *next=status_next_line(part,columns,&count);
        if(count>=sizeof(line))count=sizeof(line)-1;
        memcpy(line,part,count);line[count]=0;
        const int width=(int)count*4*font,ink=width-font;
        text(r,(r->display.width-ink)/2,y+icon+gap+(int)row*line_height,width,5*font,line,0xdedc);
        part=next;
    }
}
static bool render_stale(const osg_sample *s,uint64_t now){
    return s->source_status==OSG_FEEDBACK_STALE || (s->available &&
        (now<s->received_ms || s->source_age_ms>=OSG_STALE_MS ||
         now-s->received_ms>=OSG_STALE_MS-s->source_age_ms));
}
static bool component_changed(const osg_runtime *r, const osg_runtime *old, size_t index, uint64_t now, uint64_t before) {
    if ((r->active == (int)index) != (old->active == (int)index) || r->requested[index] != old->requested[index]) return true;
    const osg_widget_binding *binding = r->panel->widgets[index].component;
    osg_widget_state_context current, previous;
    osg_widget_runtime_context(r, index, now, &current); osg_widget_runtime_context(old, index, before, &previous);
    if (osg_widget_state_resolve(binding->definition, &current) != osg_widget_state_resolve(binding->definition, &previous)) return true;
    for (unsigned p = 0; p < binding->definition->output_count; p++) {
        const unsigned slot = binding->outputs[p].slot;
        if (memcmp(&r->samples[slot], &old->samples[slot], sizeof(osg_sample)) || (osg_widget_current_sample(&current, (int)p) != NULL) != (osg_widget_current_sample(&previous, (int)p) != NULL)) return true;
    }
    return false;
}
osg_damage osg_render_damage(const osg_runtime *r,const osg_runtime *old,uint64_t now,uint64_t before){
    osg_damage d={0};
    if(!old || !old->panel || r->panel!=old->panel || r->page!=old->page ||
       r->display.width!=old->display.width || r->display.height!=old->display.height ||
       r->screen!=old->screen || strcmp(r->screen_message,old->screen_message) || r->connected!=old->connected || r->armed!=old->armed ||
       r->arming!=old->arming || r->live_input!=old->live_input){d.full=true;return d;}
    for(size_t i=0;i<r->panel->widget_count;i++){
        const osg_widget *w=&r->panel->widgets[i];
        if(w->page!=r->page)continue;
        bool changed=w->kind==OSG_COMPONENT
            ? component_changed(r,old,i,now,before)
            : w->kind==OSG_BUTTON
            ? ((r->active==(int)i)!=(old->active==(int)i))
            : (memcmp(&r->samples[i],&old->samples[i],sizeof(osg_sample))!=0 ||
               render_stale(&r->samples[i],now)!=render_stale(&old->samples[i],before));
        if(changed)d.widgets|=UINT32_C(1)<<i;
    }
    return d;
}
void osg_render_widget(osg_runtime *r, size_t i, uint64_t now) {
    static const char *provenance[] = {"REQUESTED", "LOCAL", "HOST ACCEPTED", "HOST SUBMITTED", "GAME CONFIRMED"};
    if (!r || !r->panel || i >= r->panel->widget_count || r->screen != OSG_SCREEN_PANEL) return;
    const osg_widget *w = &r->panel->widgets[i]; if (w->page != r->page || w->kind==OSG_COMPONENT) return;
    int x, y, width, height; geometry(r, w, &x, &y, &width, &height);
    if(w->kind==OSG_IMAGE){
        if(w->pixels && w->image_width && w->image_height){
            if(r->display.image){r->display.image(r->display.context,x,y,width,height,w->pixels,w->image_width,w->image_height,w->background);return;}
            for(int iy=0;iy<height;iy++)for(int ix=0;ix<width;){
                const unsigned char *p=w->pixels+3*((size_t)(iy*w->image_height/height)*w->image_width+ix*w->image_width/width);
                uint16_t color=(uint16_t)(p[0]|p[1]<<8);const unsigned alpha=p[2];
                int run=1;
                while(ix+run<width){const unsigned char *q=w->pixels+3*((size_t)(iy*w->image_height/height)*w->image_width+(ix+run)*w->image_width/width);if(q[0]!=p[0]||q[1]!=p[1]||q[2]!=p[2])break;run++;}
                if(alpha){
                    if(alpha<255){const unsigned bg=w->background;const unsigned red=(((color>>11)&31)*alpha+((bg>>11)&31)*(255-alpha))/255,green=(((color>>5)&63)*alpha+((bg>>5)&63)*(255-alpha))/255,blue=((color&31)*alpha+(bg&31)*(255-alpha))/255;color=(uint16_t)(red<<11|green<<5|blue);}
                    fill(r,x+ix,y+iy,run,1,color);
                }
                ix+=run;
            }
        }
        return;
    }
    fill(r, x, y, width, height, w->background);
    text(r, x + 4, y + 3, width - 8, height < 20 ? height - 6 : 17, w->label, w->foreground);
    if (w->kind == OSG_BUTTON) {
        fill(r, x + 4, y + 24, width - 8, height - 28, r->active == (int)i ? w->accent : 0x2128);
        text(r, x + 8, y + 28, width - 16, height - 32, r->armed ? "PRESS" : "DISARMED", w->foreground);
        return;
    }
    const osg_sample *s = &r->samples[i];
    const bool known = s->available && r->connected && s->source_status != OSG_FEEDBACK_UNKNOWN && s->source_status != OSG_FEEDBACK_DISCONNECTED;
    const bool stale = s->source_status == OSG_FEEDBACK_STALE || (known && (now < s->received_ms || now - s->received_ms >= OSG_STALE_MS ||
        s->source_age_ms >= OSG_STALE_MS || now - s->received_ms >= OSG_STALE_MS - s->source_age_ms));
    char value[384], status[64];
    if (!known) (void)snprintf(value, sizeof(value), "%s", !r->connected || s->source_status == OSG_FEEDBACK_DISCONNECTED ? "DISCONNECTED" : stale ? "STALE" : "UNKNOWN");
    else if (w->kind == OSG_TEXT) (void)snprintf(value, sizeof(value), "%.120s%.120s%.120s", w->prefix, s->text, w->suffix);
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
void osg_render(osg_runtime *r, uint64_t now) {
    fill(r, 0, 0, r->display.width, r->display.height, 0x08c4);
    if(r->screen!=OSG_SCREEN_PANEL){status_screen(r);return;}
    for(int layer=0;layer<2;layer++)for(size_t i=0;i<r->panel->widget_count;i++)
        if((r->panel->widgets[i].kind==OSG_IMAGE)==(layer==0))osg_render_widget(r,i,now);
}
