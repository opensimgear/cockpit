#include "panel_source.h"
#include <math.h>
#include <stdio.h>
#include <string.h>

typedef struct {
    osg_widget_arena *arena;
    osg_panel_source_diagnostic *error;
    const osg_panel_source_limits *limits;
    const cJSON *ui, *presentation, *pages, *controls, *feedback, *assets, *widgets;
    uint32_t controls_used, outputs_used, slots;
    size_t bitmaps;
} compiler;
static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static const char *str(const cJSON *v, const char *key) { return cJSON_GetStringValue(get(v, key)); }
static double num(const cJSON *v) { return cJSON_IsNumber(v) ? v->valuedouble : NAN; }
static double number(const cJSON *v, const char *key) { return num(get(v, key)); }
static bool same(const char *a, const char *b) { return a && b && !strcmp(a, b); }
static bool fail(compiler *c, osg_panel_source_error code, const char *field) {
    if (c->error->code == OSG_PANEL_SOURCE_OK) {
        c->error->code = code; (void)snprintf(c->error->field, sizeof(c->error->field), "%s", field);
    }
    return false;
}
static bool invalid(compiler *c, const char *field) { return fail(c, OSG_PANEL_SOURCE_INVALID, field); }
static bool budget(compiler *c, const char *field, size_t required, size_t available) {
    if (c->error->code == OSG_PANEL_SOURCE_OK) { c->error->required = required; c->error->available = available; }
    return fail(c, OSG_PANEL_SOURCE_BUDGET, field);
}
static bool key_in(const char *keys, const char *key) {
    if (!key) return false;
    const size_t length = strlen(key);
    for (const char *p = keys; *p;) {
        const char *end = strchr(p, '|'); if (!end) end = p + strlen(p);
        if ((size_t)(end - p) == length && !memcmp(p, key, length)) return true;
        p = *end ? end + 1 : end;
    }
    return false;
}
static bool object(compiler *c, const cJSON *v, const char *required, const char *optional, const char *field) {
    if (!cJSON_IsObject(v)) return invalid(c, field);
    for (const cJSON *p = v->child; p; p = p->next) {
        if (!key_in(required, p->string) && !key_in(optional, p->string)) return invalid(c, field);
        for (const cJSON *before = v->child; before != p; before = before->next) if (same(before->string, p->string)) return invalid(c, field);
    }
    for (const char *p = required; *p;) {
        const char *end = strchr(p, '|'); if (!end) end = p + strlen(p);
        bool found = false;
        for (const cJSON *item = v->child; item; item = item->next)
            if (item->string && (size_t)(end - p) == strlen(item->string) && !memcmp(p, item->string, (size_t)(end - p))) found = true;
        if (!found) return invalid(c, field);
        p = *end ? end + 1 : end;
    }
    return true;
}
static bool array(compiler *c, const cJSON *v, size_t min, size_t max, const char *field) {
    if (!cJSON_IsArray(v) || (size_t)cJSON_GetArraySize(v) < min) return invalid(c, field);
    const size_t count = (size_t)cJSON_GetArraySize(v);
    return count <= max || budget(c, field, count, max);
}
static bool bounded_number(const cJSON *v, double min, double max, bool integer) {
    const double n = num(v); return isfinite(n) && n >= min && n <= max && (!integer || n == floor(n));
}
static bool title(const char *s) { return osg_widget_text_valid(s) && !osg_widget_text_blank(s); }
static bool bounds(compiler *c, const cJSON *v, osg_bounds *out) {
    if (!object(c, v, "x|y|width|height", "", "bounds")) return false;
    *out = (osg_bounds){number(v,"x"), number(v,"y"), number(v,"width"), number(v,"height")};
    return (bounded_number(get(v,"x"),0,1,false) && bounded_number(get(v,"y"),0,1,false) && bounded_number(get(v,"width"),0,1,false) && bounded_number(get(v,"height"),0,1,false)
        && out->width > 0 && out->height > 0 && out->x + out->width <= 1 && out->y + out->height <= 1) || invalid(c, "bounds");
}
static bool contains(osg_bounds a, osg_bounds b) { return b.x >= a.x - 1e-9 && b.y >= a.y - 1e-9 && b.x + b.width <= a.x + a.width + 1e-9 && b.y + b.height <= a.y + a.height + 1e-9; }
static bool overlaps(osg_bounds a, osg_bounds b) { return fmin(a.x+a.width,b.x+b.width)-fmax(a.x,b.x)>1e-9 && fmin(a.y+a.height,b.y+b.height)-fmax(a.y,b.y)>1e-9; }
static int index_of(const cJSON *list, const char *id) {
    int i = 0; for (const cJSON *v = list ? list->child : NULL; v; v = v->next, i++) if (same(str(v,"id"),id)) return i;
    return -1;
}
static const cJSON *find(const cJSON *list, const char *id) { return cJSON_GetArrayItem(list, index_of(list, id)); }
static void *allocate(compiler *c, size_t bytes) {
    const size_t align = _Alignof(max_align_t), pad = (align - c->arena->used % align) % align;
    if (pad > c->arena->capacity - c->arena->used || bytes > c->arena->capacity - c->arena->used - pad) {
        (void)budget(c, "arenaBytes", c->arena->used + pad + bytes, c->arena->capacity); return NULL;
    }
    void *out = c->arena->data + c->arena->used + pad; memset(out, 0, bytes); c->arena->used += pad + bytes; return out;
}
static const char *save(compiler *c, const char *s) {
    if (!s) { (void)invalid(c, "string"); return NULL; }
    char *out = allocate(c, strlen(s)+1); if (out) memcpy(out,s,strlen(s)+1); return out;
}
static bool digest(const char *s) { if (!s || strlen(s)!=64) return false; for (unsigned i=0;i<64;i++) if (!((s[i]>='0'&&s[i]<='9')||(s[i]>='a'&&s[i]<='f'))) return false; return true; }
static const char *asset_hash(compiler *c, const char *asset_id) {
    const cJSON *asset = find(c->assets,asset_id); if (!asset) { (void)invalid(c,"assetId"); return NULL; }
    char hash[72]; memcpy(hash,"sha256:",7); memcpy(hash+7,str(asset,"sha256"),65); return save(c,hash);
}
static bool color(const cJSON *v, uint16_t *out) {
    const char *s = cJSON_GetStringValue(v); if (!s || strlen(s)!=7 || *s!='#') return false;
    unsigned value=0; for(unsigned i=1;i<7;i++) { const char ch=s[i]; unsigned digit;
        if(ch>='0'&&ch<='9')digit=(unsigned)(ch-'0'); else if(ch>='a'&&ch<='f')digit=(unsigned)(ch-'a'+10); else if(ch>='A'&&ch<='F')digit=(unsigned)(ch-'A'+10); else return false;
        value=value<<4|digit;
    }
    if(out)*out=(uint16_t)((value>>19&31)<<11|(value>>10&63)<<5|(value>>3&31));
    return true;
}
static bool style(compiler *c, const cJSON *v) {
    if(!v)return true;
    if(!object(c,v,"","foreground|background|accent","style"))return false;
    for(const cJSON *p=v->child;p;p=p->next)if(!color(p,NULL))return invalid(c,"style.color");
    return true;
}
/* Validate global IDs without retaining JSON pointers in the immutable scene. */
static bool unique_ids(compiler *c, const cJSON *targets) {
    const cJSON *lists[]={c->controls,c->feedback,c->pages,c->assets,c->widgets,get(c->presentation,"groups"),targets};
    for(size_t list=0;list<sizeof(lists)/sizeof(*lists);list++)for(const cJSON *v=lists[list]->child;v;v=v->next) {
        if(!osg_stable_id(str(v,"id")))return invalid(c,"id");
        for(size_t prior=0;prior<=list;prior++)for(const cJSON *p=lists[prior]->child;p&&p!=v;p=p->next)if(same(str(p,"id"),str(v,"id")))return invalid(c,"duplicateId");
    }
    for(const cJSON *vp=get(c->presentation,"viewports")->child;vp;vp=vp->next)for(const cJSON *v=get(vp,"regions")?get(vp,"regions")->child:NULL;v;v=v->next) {
        if(!osg_stable_id(str(v,"id")))return invalid(c,"region.id");
        for(size_t list=0;list<sizeof(lists)/sizeof(*lists);list++)if(find(lists[list],str(v,"id")))return invalid(c,"duplicateId");
        for(const cJSON *before=get(c->presentation,"viewports")->child;before;before=before->next) {
            for(const cJSON *p=get(before,"regions")?get(before,"regions")->child:NULL;p;p=p->next) {
                if(p==v)goto checked;
                if(same(str(p,"id"),str(v,"id")))return invalid(c,"duplicateId");
            }
        }
checked:;
    }
    return true;
}
static bool primitives(compiler *c) {
    for(const cJSON *v=c->controls->child;v;v=v->next) {
        const char *kind=str(v,"kind");
        if(!object(c,v,same(kind,"axis")?"id|label|kind|axisMode|fallback":same(kind,"hat")?"id|label|kind|fallback":"id|label|kind","style","control")||!title(str(v,"label"))||!style(c,get(v,"style")))return invalid(c,"control");
        if(same(kind,"axis")) { const bool bipolar=same(str(v,"axisMode"),"bipolar"); if((!bipolar&&!same(str(v,"axisMode"),"unipolar"))||!bounded_number(get(v,"fallback"),bipolar?-1:0,1,false))return invalid(c,"control.axis"); }
        else if(same(kind,"hat")) { if(number(v,"fallback")!=-1)return invalid(c,"control.hat"); }
        else if(!same(kind,"button"))return invalid(c,"control.kind");
    }
    for(const cJSON *v=c->feedback->child;v;v=v->next)
        if(!object(c,v,"id|label|kind|channelId","style","feedback")||!title(str(v,"label"))||!osg_stable_id(str(v,"channelId"))||!key_in("text|indicator|gauge",str(v,"kind"))||!style(c,get(v,"style")))return invalid(c,"feedback");
    size_t bytes=0;
    for(const cJSON *v=c->assets->child;v;v=v->next) {
        if(!object(c,v,"id|sha256|sizeBytes|mime","","asset")||!digest(str(v,"sha256"))||!key_in("image/png|image/jpeg|image/webp",str(v,"mime"))||!bounded_number(get(v,"sizeBytes"),1,2*1024*1024,true))return invalid(c,"asset");
        bytes+=(size_t)number(v,"sizeBytes");
    }
    return bytes<=8*1024*1024||invalid(c,"asset.totalBytes");
}
static const cJSON *viewport(compiler *c, const char *page) {
    for(const cJSON *v=get(c->presentation,"viewports")->child;v;v=v->next)if(same(str(v,"pageId"),page))return v;
    return NULL;
}
static bool pages(compiler *c, osg_scene *image) {
    for(const cJSON *v=get(c->presentation,"viewports")->child;v;v=v->next) {
        if(!object(c,v,"pageId|width|height","safeInsets|backgroundAssetId|regions","viewport")||index_of(c->pages,str(v,"pageId"))<0||!bounded_number(get(v,"width"),320,8192,true)||!bounded_number(get(v,"height"),320,8192,true))return invalid(c,"viewport");
        for(const cJSON *p=get(c->presentation,"viewports")->child;p!=v;p=p->next)if(same(str(p,"pageId"),str(v,"pageId")))return invalid(c,"viewport.pageId");
        if(get(v,"backgroundAssetId"))return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"viewport.backgroundAssetId");
        const cJSON *insets=get(v,"safeInsets");
        if(insets&&(!object(c,insets,"top|right|bottom|left","","safeInsets")||!bounded_number(get(insets,"top"),0,.25,false)||!bounded_number(get(insets,"bottom"),0,.25,false)||!bounded_number(get(insets,"left"),0,.25,false)||!bounded_number(get(insets,"right"),0,.25,false)||number(insets,"left")+number(insets,"right")>=.5||number(insets,"top")+number(insets,"bottom")>=.5))return invalid(c,"safeInsets");
        const cJSON *regions=get(v,"regions"); if(regions) {
            if(!array(c,regions,1,32,"regions"))return false;
            for(const cJSON *region=regions->child;region;region=region->next) {
                osg_bounds b; if(!object(c,region,"id|label|bounds","","region")||!osg_stable_id(str(region,"id"))||!title(str(region,"label"))||!bounds(c,get(region,"bounds"),&b))return invalid(c,"region");
                for(const cJSON *before=regions->child;before!=region;before=before->next) { osg_bounds a; if(!bounds(c,get(before,"bounds"),&a)||overlaps(a,b))return invalid(c,"region.overlap"); }
            }
        }
    }
    size_t i=0;
    for(const cJSON *v=c->pages->child;v;v=v->next,i++) {
        if(!object(c,v,"id|title|items","","page")||!title(str(v,"title"))||!array(c,get(v,"items"),0,64,"page.items"))return invalid(c,"page");
        const cJSON *vp=viewport(c,str(v,"id")); if(!vp)return invalid(c,"viewport.pageId");
        image->pages[i]=(osg_page){save(c,str(v,"id")),save(c,str(v,"title")),(uint16_t)number(vp,"width"),(uint16_t)number(vp,"height")};
    }
    for(const cJSON *g=get(c->presentation,"groups")->child;g;g=g->next)
        if(!object(c,g,"id|pageId|label","","group")||!title(str(g,"label"))||!find(c->pages,str(g,"pageId")))return invalid(c,"group");
    return c->error->code==OSG_PANEL_SOURCE_OK;
}
static bool widget_bounds(compiler *c, const cJSON *v, osg_widget *w, const osg_scene *image) {
    osg_bounds source; if(!bounds(c,get(v,"bounds"),&source))return false;
    const int p=index_of(c->pages,str(v,"pageId")); if(p<0)return invalid(c,"widget.pageId"); w->page=(uint8_t)p;
    const cJSON *vp=viewport(c,str(v,"pageId")),*regions=get(vp,"regions"),*insets=get(vp,"safeInsets");
    bool fits=!regions;
    for(const cJSON *r=regions?regions->child:NULL;r;r=r->next) { osg_bounds b; if(!bounds(c,get(r,"bounds"),&b))return false; if(contains(b,source))fits=true; }
    if(!fits)return invalid(c,"widget.viewport");
    const double left=insets?number(insets,"left"):0,top=insets?number(insets,"top"):0,right=insets?number(insets,"right"):0,bottom=insets?number(insets,"bottom"):0;
    w->bounds=(osg_bounds){left+source.x*(1-left-right),top+source.y*(1-top-bottom),source.width*(1-left-right),source.height*(1-top-bottom)};
    if(w->kind==OSG_BUTTON||(w->kind==OSG_COMPONENT&&w->component->definition->interaction!=OSG_INTERACTION_NONE)) {
        const osg_bounds hit=w->kind==OSG_COMPONENT?w->component->definition->hit_area:(osg_bounds){0,0,1,1};
        const double scale=fmin((double)c->limits->width/image->pages[p].width,(double)c->limits->height/image->pages[p].height);
        const osg_bounds artwork=osg_widget_artwork_bounds(w,&image->pages[p]);
        const double width=floor(artwork.width*image->pages[p].width*scale*hit.width),height=floor(artwork.height*image->pages[p].height*scale*hit.height);
        if(width<44||height<44)return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"hitArea.minimum44Pixels");
    }
    return true;
}
static bool used_reference(compiler *c, const cJSON *refs, const cJSON *primitives, size_t count, uint32_t *used, const char *field) {
    if(!array(c,refs,count,count,field))return false;
    for(const cJSON *v=refs->child;v;v=v->next) {
        const int index=index_of(primitives,cJSON_GetStringValue(v));
        if(index<0||(*used&(UINT32_C(1)<<index)))return invalid(c,field);
        *used|=UINT32_C(1)<<index;
    }
    return true;
}
static bool definition(compiler *c,const cJSON *source,const cJSON *properties,const osg_widget_component **out) {
    const cJSON defaults={.type=cJSON_Object}; if(!properties)properties=&defaults;
    osg_widget_diagnostic error;
    if(osg_widget_source_compile(source,properties,c->assets,c->arena,out,&error))return true;
    memcpy(c->error->definition,error.definition,sizeof(error.definition));memcpy(c->error->layer,error.layer,sizeof(error.layer));
    if(error.code==OSG_WIDGET_SOURCE_BUDGET)return budget(c,error.field,error.required,error.available);
    return invalid(c,error.field);
}
static bool component(compiler *c, const cJSON *v, osg_widget *w) {
    const cJSON *config=get(v,"config"),*defs=get(c->presentation,"definitions"),*source=NULL;
    if(!object(c,config,"definitionId|definitionVersion|properties","","component.config"))return false;
    if(!cJSON_IsArray(defs))return invalid(c,"component.definitionRevision");
    for(const cJSON *d=defs->child;d;d=d->next)if(same(str(d,"id"),str(config,"definitionId"))&&number(d,"version")==number(config,"definitionVersion"))source=d;
    if(!source)return invalid(c,"component.definitionRevision");
    osg_widget_binding *binding=allocate(c,sizeof(*binding)); if(!binding)return false;
    if(!definition(c,source,get(config,"properties"),&binding->definition))return false;
    w->kind=OSG_COMPONENT;w->component=binding;
    const osg_widget_component *d=binding->definition;
    const size_t inputs=d->interaction==OSG_INTERACTION_NONE||d->interaction==OSG_INTERACTION_NAVIGATE?0:1;
    if(!used_reference(c,get(v,"controlIds"),c->controls,inputs,&c->controls_used,"component.controlIds")||!used_reference(c,get(v,"feedbackIds"),c->feedback,d->output_count,&c->outputs_used,"component.feedbackIds"))return false;
    w->reference=save(c,inputs?cJSON_GetStringValue(cJSON_GetArrayItem(get(v,"controlIds"),0)):"");
    if(inputs) {
        const cJSON *control=find(c->controls,w->reference); const bool selector=d->interaction==OSG_INTERACTION_SELECTOR;
        if(!same(str(control,"kind"),selector?"axis":"button")||(selector&&(!same(str(control,"axisMode"),d->bipolar?"bipolar":"unipolar")||number(control,"fallback")!=d->fallback)))return invalid(c,"component.inputContract");
    }
    if(d->interaction==OSG_INTERACTION_NAVIGATE&&!find(c->pages,d->navigate_page))return invalid(c,"component.navigatePage");
    for(unsigned port=0;port<d->output_count;port++) {
        const char *id=cJSON_GetStringValue(cJSON_GetArrayItem(get(v,"feedbackIds"),(int)port));const cJSON *out=find(c->feedback,id);
        if(!same(str(out,"kind"),d->outputs[port].kind==OSG_BOOL?"indicator":d->outputs[port].kind==OSG_STRING?"text":"gauge"))return invalid(c,"component.outputContract");
        unsigned slot=0;while(slot<OSG_MAX_WIDGETS&&(c->slots&(UINT32_C(1)<<slot)))slot++;
        if(slot==OSG_MAX_WIDGETS)return budget(c,"outputSlots",slot+1,OSG_MAX_WIDGETS);
        c->slots|=UINT32_C(1)<<slot;binding->outputs[port]=(osg_bound_output){save(c,id),save(c,str(out,"channelId")),(uint8_t)slot};
    }
    unsigned images=0;for(unsigned l=0;l<d->layer_count;l++)if(d->layers[l].kind==OSG_LAYER_IMAGE&&*d->layers[l].asset_id)images++;
    if(images>c->limits->bitmap_references-c->bitmaps)return budget(c,"bitmapReferences",c->bitmaps+images,c->limits->bitmap_references);
    if(images) { binding->bitmaps=allocate(c,images*sizeof(*binding->bitmaps));if(!binding->bitmaps)return false; }
    for(unsigned l=0;l<d->layer_count;l++)if(d->layers[l].kind==OSG_LAYER_IMAGE&&*d->layers[l].asset_id) {
        const osg_widget_layer *layer=&d->layers[l];osg_bound_bitmap *bitmap=&binding->bitmaps[binding->bitmap_count++];
        bitmap->layer_id=layer->id;bitmap->asset_id=layer->asset_id;bitmap->hash=asset_hash(c,layer->asset_id);bitmap->bounds=layer->bounds;
    }
    c->bitmaps+=images;return c->error->code==OSG_PANEL_SOURCE_OK;
}
static bool legacy(compiler *c, const cJSON *v, osg_widget *w, osg_scene *image, size_t i) {
    const char *kind=str(v,"kind");const cJSON *config=get(v,"config");
    if(same(kind,"button"))w->kind=OSG_BUTTON;else if(same(kind,"indicator"))w->kind=OSG_INDICATOR;else if(same(kind,"text"))w->kind=OSG_TEXT;else if(same(kind,"numeric"))w->kind=OSG_NUMERIC;else if(same(kind,"gauge"))w->kind=OSG_GAUGE;else if(same(kind,"image"))w->kind=OSG_IMAGE;
    else return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"widget.kind");
    const bool button=w->kind==OSG_BUTTON,bitmap=w->kind==OSG_IMAGE,numeric=w->kind==OSG_NUMERIC||w->kind==OSG_GAUGE;
    if(!used_reference(c,get(v,"controlIds"),c->controls,button?1:0,&c->controls_used,"widget.controlIds")||!used_reference(c,get(v,"feedbackIds"),c->feedback,button||bitmap?0:1,&c->outputs_used,"widget.feedbackIds"))return false;
    const char *id=button?cJSON_GetStringValue(cJSON_GetArrayItem(get(v,"controlIds"),0)):button||bitmap?"":cJSON_GetStringValue(cJSON_GetArrayItem(get(v,"feedbackIds"),0));
    const cJSON *primitive=find(button?c->controls:c->feedback,id);
    if(!bitmap&&!same(str(primitive,"kind"),button?"button":w->kind==OSG_INDICATOR?"indicator":w->kind==OSG_TEXT?"text":"gauge"))return invalid(c,"widget.primitive");
    w->reference=save(c,bitmap?"":button?id:str(primitive,"channelId"));w->output_id=save(c,button||bitmap?"":id);
    if(button) {
        if(!object(c,config,"buttonMode|pulseMs|repeatMs","iconAssetId","button.config"))return false;
        if(!same(str(config,"buttonMode"),"momentary")||number(config,"pulseMs")!=0||number(config,"repeatMs")!=0||get(config,"iconAssetId"))return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"button.config");
    } else if(bitmap) {
        if(!object(c,config,"","assetId","image.config"))return false;
        if(!get(config,"assetId"))return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"image.emptyAsset");
        image->image_hashes[i]=asset_hash(c,str(config,"assetId"));
        if(c->bitmaps>=c->limits->bitmap_references)return budget(c,"bitmapReferences",c->bitmaps+1,c->limits->bitmap_references);
        c->bitmaps++;
    } else if(numeric) {
        if(!object(c,config,"min|max|unit|precision","","numeric.config")||!bounded_number(get(config,"min"),-1e9,1e9,false)||!bounded_number(get(config,"max"),-1e9,1e9,false)||number(config,"min")>=number(config,"max")||!bounded_number(get(config,"precision"),0,4,true)||!osg_widget_text_valid(str(config,"unit")))return invalid(c,"numeric.config");
        w->min=number(config,"min");w->max=number(config,"max");w->precision=(uint8_t)number(config,"precision");w->unit=save(c,str(config,"unit"));
    } else if(w->kind==OSG_TEXT) {
        if(!object(c,config,"prefix|suffix","","text.config")||!osg_widget_text_valid(str(config,"prefix"))||!osg_widget_text_valid(str(config,"suffix")))return invalid(c,"text.config");
        w->prefix=save(c,str(config,"prefix"));w->suffix=save(c,str(config,"suffix"));
    } else if(!object(c,config,"","","indicator.config"))return false;
    const cJSON *paint=get(v,"style");w->foreground=0xdf3d;w->background=0x1126;w->accent=0x8718;
    if(get(paint,"foreground"))(void)color(get(paint,"foreground"),&w->foreground);
    if(get(paint,"background"))(void)color(get(paint,"background"),&w->background);
    if(get(paint,"accent"))(void)color(get(paint,"accent"),&w->accent);
    return c->error->code==OSG_PANEL_SOURCE_OK;
}
static bool page_items(compiler *c, const osg_scene *image) {
    size_t page=0;
    for(const cJSON *p=c->pages->child;p;p=p->next,page++) {
        const cJSON *item=get(p,"items")->child;bool populated=false;
        for(size_t w=0;w<image->panel.widget_count;w++)if(image->widgets[w].page==page) {
            populated=true;const cJSON *v=cJSON_GetArrayItem(c->widgets,(int)w),*lists[]={get(v,"controlIds"),get(v,"feedbackIds")};osg_bounds expected;
            if(!bounds(c,get(v,"bounds"),&expected))return false;
            for(unsigned list=0;list<2;list++)for(const cJSON *ref=lists[list]->child;ref;ref=ref->next) {
                osg_bounds actual;
                if(!object(c,item,"kind|refId|bounds","","page.item")||!same(str(item,"kind"),list?"feedback":"control")||!same(str(item,"refId"),cJSON_GetStringValue(ref))||!bounds(c,get(item,"bounds"),&actual)||actual.x!=expected.x||actual.y!=expected.y||actual.width!=expected.width||actual.height!=expected.height)return invalid(c,"page.items");
                item=item->next;
            }
        }
        if(item||!populated)return invalid(c,"page.items");
    }
    return true;
}
static bool targets(compiler *c, const cJSON *doc) {
    const cJSON *list=get(doc,"targets"),*bindings=get(doc,"bindings");
    for(const cJSON *v=list->child;v;v=v->next) {
        const cJSON *axes=get(v,"axes");
        if(!object(c,v,"id|label|axes|buttons|hats","","target")||!title(str(v,"label"))||!array(c,axes,0,16,"target.axes")||!bounded_number(get(v,"buttons"),0,128,true)||!bounded_number(get(v,"hats"),0,4,true)||cJSON_GetArraySize(axes)+number(v,"buttons")+number(v,"hats")<1)return invalid(c,"target");
        for(const cJSON *axis=axes->child;axis;axis=axis->next)if(!key_in("unipolar|bipolar",cJSON_GetStringValue(axis)))return invalid(c,"target.axisMode");
    }
    for(const cJSON *v=bindings->child;v;v=v->next) {
        if(!object(c,v,"controlId|targetId|kind|index","","binding"))return false;
        const cJSON *control=find(c->controls,str(v,"controlId")),*target=find(list,str(v,"targetId"));const char *kind=str(v,"kind");
        if(!control||!target||!same(kind,str(control,"kind")))return invalid(c,"binding.reference");
        const double max=same(kind,"axis")?cJSON_GetArraySize(get(target,"axes"))-1:same(kind,"button")?number(target,"buttons")-1:number(target,"hats")-1;
        if(!bounded_number(get(v,"index"),0,max,true))return invalid(c,"binding.index");
        if(same(kind,"axis")&&!same(cJSON_GetStringValue(cJSON_GetArrayItem(get(target,"axes"),(int)number(v,"index"))),str(control,"axisMode")))return invalid(c,"binding.axisMode");
        for(const cJSON *p=bindings->child;p!=v;p=p->next)if(same(str(p,"controlId"),str(v,"controlId"))||(same(str(p,"targetId"),str(v,"targetId"))&&same(str(p,"kind"),kind)&&number(p,"index")==number(v,"index")))return invalid(c,"binding.duplicate");
    }
    return true;
}
static bool document(compiler *c,const cJSON *doc,const char *hash,osg_scene **result) {
    c->ui=get(doc,"ui");c->presentation=get(doc,"presentation");c->pages=get(c->ui,"pages");c->controls=get(c->ui,"controls");c->feedback=get(c->ui,"feedbackWidgets");c->assets=get(c->ui,"assets");c->widgets=get(c->presentation,"widgets");
    const bool portable=number(doc,"schemaVersion")==3;
    if(!object(c,doc,"schemaVersion|ui|presentation|targets|bindings","","document")||(!portable&&number(doc,"schemaVersion")!=2)||!object(c,c->ui,"schemaVersion|id|title|controls|pages|feedbackWidgets|assets","","ui")||number(c->ui,"schemaVersion")!=1||!osg_stable_id(str(c->ui,"id"))||!title(str(c->ui,"title"))||!object(c,c->presentation,portable?"widgets|groups|viewports|grid|definitions":"widgets|groups|viewports|grid","","presentation"))return invalid(c,"document");
    if(!array(c,c->pages,1,OSG_MAX_PAGES,"pages")||!array(c,c->controls,0,OSG_MAX_WIDGETS,"controls")||!array(c,c->feedback,0,OSG_MAX_WIDGETS,"feedback")||!array(c,c->assets,0,64,"assets")||!array(c,c->widgets,1,OSG_MAX_WIDGETS,"widgets")||!array(c,get(c->presentation,"groups"),0,64,"groups")||!array(c,get(c->presentation,"viewports"),(size_t)cJSON_GetArraySize(c->pages),(size_t)cJSON_GetArraySize(c->pages),"viewports")||(portable&&!array(c,get(c->presentation,"definitions"),0,64,"definitions"))||!array(c,get(doc,"targets"),0,8,"targets")||!array(c,get(doc,"bindings"),(size_t)cJSON_GetArraySize(c->controls),(size_t)cJSON_GetArraySize(c->controls),"bindings"))return false;
    const cJSON *grid=get(c->presentation,"grid");
    if(!object(c,grid,"columns|rows|snap","","grid")||!bounded_number(get(grid,"columns"),1,128,true)||!bounded_number(get(grid,"rows"),1,128,true)||!cJSON_IsBool(get(grid,"snap")))return invalid(c,"grid");
    if(!unique_ids(c,get(doc,"targets"))||!primitives(c)||!targets(c,doc))return false;
    /* Even unused library revisions must be valid. Rewind only this validation
     * scratch before admitting instances, without charging unused layers twice. */
    const osg_widget_arena mark=*c->arena;const cJSON *definitions=get(c->presentation,"definitions");
    for(const cJSON *d=definitions?definitions->child:NULL;d;d=d->next) {
        for(const cJSON *p=definitions->child;p!=d;p=p->next)if(same(str(p,"id"),str(d,"id"))&&number(p,"version")==number(d,"version"))return invalid(c,"definition.duplicateRevision");
        const osg_widget_component *temporary; if(!definition(c,d,NULL,&temporary))return false; *c->arena=mark;
    }
    osg_scene *image=allocate(c,sizeof(*image));if(!image)return false;
    image->panel=(osg_panel){save(c,str(c->ui,"id")),save(c,hash),image->pages,image->widgets,(size_t)cJSON_GetArraySize(c->pages),(size_t)cJSON_GetArraySize(c->widgets),NULL};
    if(!pages(c,image))return false;
    /* Legacy feedback slots remain at their widget index. Components receive
     * the remaining slots independently of widget position or logical ID. */
    for(size_t i=0;i<image->panel.widget_count;i++) { const char *kind=str(cJSON_GetArrayItem(c->widgets,(int)i),"kind"); if(key_in("indicator|text|numeric|gauge",kind))c->slots|=UINT32_C(1)<<i; }
    size_t i=0;
    for(const cJSON *v=c->widgets->child;v;v=v->next,i++) {
        if(osg_stable_id(str(v,"id")))(void)snprintf(c->error->widget,sizeof(c->error->widget),"%s",str(v,"id"));
        osg_widget *w=&image->widgets[i];
        if(!object(c,v,"id|pageId|kind|label|bounds|controlIds|feedbackIds|config","groupId|style","widget")||!title(str(v,"label"))||!style(c,get(v,"style")))return invalid(c,"widget");
        const cJSON *group=get(v,"groupId")?find(get(c->presentation,"groups"),str(v,"groupId")):NULL;
        if(get(v,"groupId")&&(!group||!same(str(group,"pageId"),str(v,"pageId"))))return invalid(c,"widget.groupId");
        w->id=save(c,str(v,"id"));w->label=save(c,str(v,"label"));w->output_id=w->reference=w->prefix=w->suffix=w->unit="";w->max=1;
        if(same(str(v,"kind"),"component")) { if(!component(c,v,w))return false; }
        else if(!legacy(c,v,w,image,i))return false;
        if(!widget_bounds(c,v,w,image))return false;
    }
    const unsigned controls=(unsigned)cJSON_GetArraySize(c->controls),outputs=(unsigned)cJSON_GetArraySize(c->feedback);
    if(c->controls_used!=(controls==32?UINT32_MAX:(UINT32_C(1)<<controls)-1)||c->outputs_used!=(outputs==32?UINT32_MAX:(UINT32_C(1)<<outputs)-1))return invalid(c,"unboundPrimitive");
    if(!page_items(c,image))return false;
    const cJSON *groups=get(c->presentation,"groups");
    if(cJSON_GetArraySize(groups)) {
        osg_panel_metadata *metadata=allocate(c,sizeof(*metadata));if(!metadata)return false;
        osg_widget_group *resolved=allocate(c,(size_t)cJSON_GetArraySize(groups)*sizeof(*resolved));if(!resolved)return false;
        metadata->groups=resolved;metadata->group_count=(size_t)cJSON_GetArraySize(groups);image->panel.metadata=metadata;
        size_t g=0;
        for(const cJSON *group=groups->child;group;group=group->next,g++) {
            resolved[g]=(osg_widget_group){save(c,str(group,"id")),save(c,str(group,"label")),(uint8_t)index_of(c->pages,str(group,"pageId")),0};
            for(size_t w=0;w<image->panel.widget_count;w++)if(same(str(cJSON_GetArrayItem(c->widgets,(int)w),"groupId"),str(group,"id")))resolved[g].members|=UINT32_C(1)<<w;
        }
    }
    if(!osg_panel_valid(&image->panel))return fail(c,OSG_PANEL_SOURCE_UNSUPPORTED,"panel.geometryOrLegacyGlyphs");
    if(c->error->code!=OSG_PANEL_SOURCE_OK)return false;
    *result=image;return true;
}
bool osg_panel_source_compile(const cJSON *publication,const char *expected,osg_crypto crypto,const osg_panel_source_limits *limits,osg_widget_arena *arena,osg_scene **result,osg_panel_source_diagnostic *diagnostic) {
    osg_panel_source_diagnostic local={0};if(!diagnostic)diagnostic=&local;memset(diagnostic,0,sizeof(*diagnostic));if(result)*result=NULL;
    compiler c={.arena=arena,.error=diagnostic,.limits=limits};
    if(!result||!arena||!arena->data||(uintptr_t)arena->data%_Alignof(max_align_t)||arena->used>arena->capacity||!limits||!limits->document_bytes||limits->document_bytes>1024*1024||limits->width<64||limits->height<64||limits->width>4096||limits->height>4096||!crypto.sha256)return invalid(&c,"arguments");
    const char *canonical=str(publication,"canonicalDocument"),*hash=str(publication,"publicationHash");char actual[72];
    if(!canonical||!osg_hash_id(hash)||!same(hash,expected)||!crypto.sha256(canonical,strlen(canonical),actual)||strcmp(actual,hash))return fail(&c,OSG_PANEL_SOURCE_IDENTITY,"publicationHash");
    if(strlen(canonical)>limits->document_bytes)return budget(&c,"documentBytes",strlen(canonical),limits->document_bytes);
    if(!array(&c,get(publication,"assets"),0,0,"publication.assets"))return false;
    cJSON *doc=osg_json_parse(canonical,strlen(canonical));
    if(!doc) {
        const osg_json_diagnostic error=osg_json_last_failure();
        if(error.code==OSG_JSON_HEAP||error.code==OSG_JSON_NODES||error.code==OSG_JSON_BYTES)return budget(&c,error.code==OSG_JSON_HEAP?"json.heapBytes":error.code==OSG_JSON_NODES?"json.nodes":"json.replyBytes",error.required,error.available);
        return invalid(&c,"document.json");
    }
    const osg_widget_arena mark=*arena;bool ok=document(&c,doc,hash,result);
    if(!ok) { *arena=mark;*result=NULL; } else memset(diagnostic,0,sizeof(*diagnostic));
    cJSON_Delete(doc);return ok;
}
