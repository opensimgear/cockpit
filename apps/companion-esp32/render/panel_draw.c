#include "panel_draw.h"
#include <math.h>
#include <stdio.h>
#include <string.h>

static uint32_t rgb(uint16_t value) {
    return (uint32_t)((value >> 11) * 255 / 31) << 16 |
        (uint32_t)(((value >> 5) & 63) * 255 / 63) << 8 | (value & 31) * 255 / 31;
}
static bool fail(osg_panel_draw *draw, osg_widget_draw_error code, const char *widget, const char *layer, const char *field, size_t required, size_t available) {
    draw->diagnostic = (osg_panel_draw_diagnostic){.code=code,.required=required,.available=available};
    (void)snprintf(draw->diagnostic.widget,sizeof(draw->diagnostic.widget),"%s",widget?widget:"");
    (void)snprintf(draw->diagnostic.layer,sizeof(draw->diagnostic.layer),"%s",layer?layer:"");
    (void)snprintf(draw->diagnostic.field,sizeof(draw->diagnostic.field),"%s",field?field:"");
    return false;
}
static lv_area_t area(const osg_runtime *frame, size_t index) {
    int x,y,w,h; osg_widget_rect(frame,index,&x,&y,&w,&h);
    return (lv_area_t){x,y,x+w-1,y+h-1};
}
static bool group_area(const osg_runtime *frame, const osg_widget_group *group, lv_area_t *out) {
    bool found=false;
    for(size_t i=0;i<frame->panel->widget_count;i++)if((group->members&(UINT32_C(1)<<i))&&frame->panel->widgets[i].page==group->page) {
        lv_area_t box=area(frame,i);
        if(!found)*out=box;
        else { out->x1=LV_MIN(out->x1,box.x1);out->y1=LV_MIN(out->y1,box.y1);out->x2=LV_MAX(out->x2,box.x2);out->y2=LV_MAX(out->y2,box.y2); }
        found=true;
    }
    return found;
}
static bool text_check(osg_panel_draw *draw, const char *widget, const osg_widget_layer *layer, const osg_widget_state_context *context, unsigned state) {
    char text[OSG_WIDGET_DRAW_TEXT_BYTES];osg_widget_text_metrics metrics;
    if(!osg_widget_layer_text(layer,context,(osg_widget_state)state,text,sizeof(text))||
       !osg_widget_text_measure(&draw->fonts,(osg_widget_font)layer->font,layer->font_size,text,&metrics))
        return fail(draw,OSG_WIDGET_DRAW_TEXT,widget,layer->id,"text",0,0);
    return true;
}
bool osg_panel_draw_check(osg_panel_draw *draw, const osg_runtime *frame, uint64_t now, bool all_states) {
    if(!draw)return false;
    memset(&draw->diagnostic,0,sizeof(draw->diagnostic));
    if(!draw->backend.display||!frame||!frame->panel||frame->page>=frame->panel->page_count||
       frame->display.width!=draw->backend.board.width||frame->display.height!=draw->backend.board.height)
        return fail(draw,OSG_WIDGET_DRAW_ARGUMENT,NULL,NULL,"display",0,0);
    if(frame->screen!=OSG_SCREEN_PANEL&&!all_states)return true;
    const osg_panel_metadata *metadata=frame->panel->metadata;
    if(metadata)for(size_t i=0;i<metadata->group_count;i++) {
        const osg_widget_group *group=&metadata->groups[i];osg_widget_text_metrics metrics;lv_area_t box;
        if((all_states||group->page==frame->page)&&group_area(frame,group,&box)&&
           !osg_widget_text_measure(&draw->fonts,OSG_FONT_SANS,9,group->label,&metrics))
            return fail(draw,OSG_WIDGET_DRAW_TEXT,group->id,NULL,"group.label",0,0);
    }
    for(size_t i=0;i<frame->panel->widget_count;i++) {
        const osg_widget *widget=&frame->panel->widgets[i];if(!all_states&&widget->page!=frame->page)continue;
        if(widget->kind==OSG_IMAGE) {
            if(!widget->pixels||!widget->image_width||!widget->image_height)return fail(draw,OSG_WIDGET_DRAW_ASSET,widget->id,NULL,"image",0,0);
            continue;
        }
        if(widget->kind!=OSG_COMPONENT)continue;
        const osg_widget_binding *binding=widget->component;const osg_widget_component *definition=binding?binding->definition:NULL;
        if(!definition||!definition->width||!definition->height)return fail(draw,OSG_WIDGET_DRAW_ARGUMENT,widget->id,NULL,"definition",0,0);
        lv_area_t box=area(frame,i);
        if(lv_area_get_width(&box)<1||lv_area_get_height(&box)<1)return fail(draw,OSG_WIDGET_DRAW_ARGUMENT,widget->id,NULL,"geometry.minimumPixels",1,0);
        const double sx=(double)lv_area_get_width(&box)/definition->width,sy=(double)lv_area_get_height(&box)/definition->height;
        if(sx>32||sy>32)return fail(draw,OSG_WIDGET_DRAW_BUDGET,widget->id,NULL,"scale",(size_t)ceil(fmax(sx,sy)),32);
        osg_widget_state_context context;osg_widget_runtime_context(frame,i,now,&context);
        const unsigned current=(unsigned)osg_widget_state_resolve(definition,&context);
        for(unsigned l=0;l<definition->layer_count;l++) {
            const osg_widget_layer *layer=&definition->layers[l];
            for(unsigned state=0;state<OSG_WIDGET_SOURCE_STATES;state++) {
                if(!all_states&&state!=current)continue;
                if(!layer->paints[state].visible||layer->paints[state].opacity<=0)continue;
                if(layer->kind==OSG_LAYER_TEXT){
                    if(layer->font_size>draw->max_font_size)return fail(draw,OSG_WIDGET_DRAW_BUDGET,widget->id,layer->id,"fontSize",layer->font_size,draw->max_font_size);
                    if(!text_check(draw,widget->id,layer,&context,state))return false;
                }
                if(layer->kind==OSG_LAYER_IMAGE&&layer->asset_id&&*layer->asset_id) {
                    osg_widget_bitmap bitmap;const unsigned w=(unsigned)lround(layer->bounds.width*lv_area_get_width(&box)),h=(unsigned)lround(layer->bounds.height*lv_area_get_height(&box));
                    if(!osg_widget_bound_bitmap((void *)binding,layer->asset_id,w,h,&bitmap))return fail(draw,OSG_WIDGET_DRAW_ASSET,widget->id,layer->id,"asset",0,0);
                }
            }
        }
    }
    return true;
}
static void fill(void *context,int x,int y,int w,int h,uint16_t color) {
    osg_panel_draw *draw=context;if(w<=0||h<=0)return;
    osg_lvgl_rect(draw->layer,(lv_area_t){x,y,x+w-1,y+h-1},rgb(color),OSG_LVGL_TRANSPARENT,0,0,255);
    /* The compact diagnostic font can queue many small rectangles. Finish
     * each run so its task memory is independent of label/page length. */
    while(draw->layer->draw_task_head)lv_draw_dispatch();
}
static bool diagnostic_pixels(void *context,int x,int y,int w,int h,const uint16_t *pixels) {
    osg_panel_draw *draw=context;lv_area_t clip=draw->layer->_clip_area;
    if(y+h<=clip.y1||y>clip.y2||x+w<=clip.x1||x>clip.x2)return true;
    for(int row=0;row<h;row++)for(int col=0;col<w;) {
        int end=col+1;while(end<w&&pixels[row*w+end]==pixels[row*w+col])end++;
        fill(draw,x+col,y+row,end-col,1,pixels[row*w+col]);col=end;
    }
    return true;
}
static void text(void *context,int x,int y,int w,int h,const char *value,uint16_t color) {
    osg_panel_draw *draw=context;osg_display font=osg_raster_display(&draw->diagnostic_font,draw->frame->display.width,draw->frame->display.height);
    font.text(font.context,x,y,w,h,value,color);
}
static void line(lv_layer_t *layer,int x1,int y1,int x2,int y2) {
    lv_draw_line_dsc_t d;lv_draw_line_dsc_init(&d);d.color=lv_color_hex(0x70978c);d.opa=85;d.width=1;d.dash_width=3;d.dash_gap=3;
    d.p1=(lv_point_precise_t){x1,y1};d.p2=(lv_point_precise_t){x2,y2};lv_draw_line(layer,&d);
}
static bool groups(osg_panel_draw *draw) {
    const osg_panel_metadata *metadata=draw->frame->panel->metadata;if(!metadata)return true;
    for(size_t i=0;i<metadata->group_count;i++) {
        const osg_widget_group *group=&metadata->groups[i];lv_area_t box;
        if(group->page!=draw->frame->page||!group_area(draw->frame,group,&box))continue;
        box.x1-=2;box.y1-=2;box.x2+=2;box.y2+=2;
        line(draw->layer,box.x1,box.y1,box.x2,box.y1);line(draw->layer,box.x2,box.y1,box.x2,box.y2);
        line(draw->layer,box.x1,box.y2,box.x2,box.y2);line(draw->layer,box.x1,box.y1,box.x1,box.y2);
        osg_widget_text_metrics metrics;if(!osg_widget_text_measure(&draw->fonts,OSG_FONT_SANS,9,group->label,&metrics))return false;
        lv_area_t label={box.x1+9,box.y1-11,box.x1+16+(int)metrics.width,box.y1+1};
        osg_lvgl_rect(draw->layer,label,0x0c1720,OSG_LVGL_TRANSPARENT,0,0,255);label.x1+=4;label.x2-=4;
        if(!osg_widget_text_draw(&draw->fonts,draw->layer,label,group->label,OSG_FONT_SANS,9,LV_TEXT_ALIGN_LEFT,1,1,0x9cb4aa,255))return false;
    }
    return true;
}
static bool bitmap(void *context,const char *id,unsigned width,unsigned height,osg_widget_bitmap *out) {
    const osg_widget *widget=context;(void)id;(void)width;(void)height;
    *out=(osg_widget_bitmap){widget->pixels,widget->image_width,widget->image_height};return true;
}
static bool paint(void *context,lv_layer_t *layer) {
    osg_panel_draw *draw=context;draw->layer=layer;osg_runtime *frame=draw->frame;const osg_display original=frame->display;
    frame->display=(osg_display){.context=draw,.width=original.width,.height=original.height,.fill=fill,.text=text};
    bool ok=true;
    if(frame->screen!=OSG_SCREEN_PANEL)osg_render(frame,draw->now_ms);
    else {
        bool portable=false;for(size_t i=0;i<frame->panel->widget_count;i++)portable|=frame->panel->widgets[i].kind==OSG_COMPONENT;
        fill(draw,0,0,original.width,original.height,portable?0x08a4:0x08c4);
        ok=groups(draw);
        for(size_t i=0;ok&&i<frame->panel->widget_count;i++) {
            const osg_widget *widget=&frame->panel->widgets[i];if(widget->page!=frame->page)continue;
            osg_widget_state_context state;osg_widget_runtime_context(frame,i,draw->now_ms,&state);
            osg_widget_draw_diagnostic diagnostic={0};
            if(widget->kind==OSG_COMPONENT)ok=osg_widget_draw(layer,widget->component->definition,area(frame,i),&state,&draw->fonts,osg_widget_bound_bitmap,(void *)widget->component,&diagnostic);
            else if(widget->kind==OSG_IMAGE) {
                osg_widget_layer image={.id="image",.asset_id="image",.kind=OSG_LAYER_IMAGE,.bounds={0,0,1,1},.output=-1};
                for(unsigned s=0;s<OSG_WIDGET_SOURCE_STATES;s++)image.paints[s]=(osg_widget_paint){.visible=true,.opacity=1};
                const lv_area_t box=area(frame,i);
                const osg_widget_component definition={.width=(uint16_t)lv_area_get_width(&box),.height=(uint16_t)lv_area_get_height(&box),.layers=&image,.layer_count=1,.confirmed_output=-1,.warning_output=-1,.enabled_output=-1};
                ok=osg_widget_draw(layer,&definition,area(frame,i),&state,&draw->fonts,bitmap,(void *)widget,&diagnostic);
            } else osg_render_widget(frame,i,draw->now_ms);
            if(!ok)fail(draw,diagnostic.code,widget->id,diagnostic.layer,"draw",diagnostic.required,diagnostic.available);
        }
        if(ok&&frame->screen_message[0]){
            fill(draw,0,original.height-22,original.width,22,0x3146);
            text(draw,6,original.height-18,original.width-12,15,frame->screen_message,0xffe0);
        }
    }
    frame->display=original;draw->layer=NULL;return ok;
}
bool osg_panel_draw_init(osg_panel_draw *draw,osg_board board,void *buffer,size_t bytes) {
    if(!draw)return false;
    memset(draw,0,sizeof(*draw));draw->max_font_size=128;draw->diagnostic_font.context=draw;draw->diagnostic_font.write=diagnostic_pixels;
    return osg_lvgl_init(&draw->backend,board,buffer,bytes);
}
void osg_panel_draw_destroy(osg_panel_draw *draw) {
    if(!draw||draw->backend.rendering)return;
    osg_widget_fonts_destroy(&draw->fonts);osg_lvgl_destroy(&draw->backend);draw->frame=NULL;
}
bool osg_panel_draw_render(osg_panel_draw *draw,osg_runtime *frame,osg_damage damage,uint64_t now) {
    if(!draw||draw->backend.rendering)return false;
    if(!damage.full&&!damage.widgets)return true;
    if(!osg_panel_draw_check(draw,frame,now,false))return false;
    lv_area_t regions[OSG_MAX_WIDGETS];size_t count=0;
    if(!damage.full)for(size_t i=0;i<frame->panel->widget_count;i++)if((damage.widgets&(UINT32_C(1)<<i))&&frame->panel->widgets[i].page==frame->page)regions[count++]=area(frame,i);
    draw->frame=frame;draw->now_ms=now;
    const bool ok=osg_lvgl_render_regions(&draw->backend,paint,draw,damage.full?NULL:regions,count);
    draw->frame=NULL;return ok;
}
