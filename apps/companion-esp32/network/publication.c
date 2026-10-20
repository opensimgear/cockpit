#include "wire.h"
#include <string.h>
#include <math.h>

static const cJSON *get(const cJSON *v, const char *key) { return cJSON_GetObjectItemCaseSensitive(v, key); }
static bool same(const char *a, const char *b) { return a && b && !strcmp(a, b); }
static double number(const cJSON *v, const char *key) { const cJSON *n = get(v, key); return cJSON_IsNumber(n) ? n->valuedouble : NAN; }
static bool size(const cJSON *v, size_t min, size_t max) { return cJSON_IsArray(v) && (size_t)cJSON_GetArraySize(v) >= min && (size_t)cJSON_GetArraySize(v) <= max; }
static const char *save(osg_image *image, const char *s) {
    if (!s || strlen(s) + 1 > sizeof(image->strings) - image->used) return NULL;
    char *p = image->strings + image->used; memcpy(p, s, strlen(s) + 1); image->used += strlen(s) + 1; return p;
}
static const cJSON *find_id(const cJSON *list, const char *id) {
    for (const cJSON *v = list ? list->child : NULL; v; v = v->next) if (same(osg_string(v, "id"), id)) return v;
    return NULL;
}
static bool single_ref(const cJSON *v, const char **out, size_t n) {
    if (!size(v, n, n)) return false;
    const cJSON *first = cJSON_GetArrayItem(v, 0); *out = n ? cJSON_GetStringValue(first) : "";
    return *out != NULL;
}
static bool rgb(const cJSON *style, const char *key, uint16_t fallback, uint16_t *out) {
    const cJSON *field = get(style, key); if (!field) { *out = fallback; return true; }
    const char *s = cJSON_GetStringValue(field); if (!s || strlen(s) != 7 || s[0] != '#') return false;
    unsigned value = 0;
    for (int i = 1; i < 7; i++) {
        unsigned digit;
        if (s[i] >= '0' && s[i] <= '9') digit = (unsigned)(s[i] - '0');
        else if (s[i] >= 'a' && s[i] <= 'f') digit = (unsigned)(s[i] - 'a' + 10);
        else if (s[i] >= 'A' && s[i] <= 'F') digit = (unsigned)(s[i] - 'A' + 10);
        else return false;
        value = (value << 4) | digit;
    }
    *out = (uint16_t)(((value >> 19 & 31) << 11) | ((value >> 10 & 63) << 5) | (value >> 3 & 31)); return true;
}
bool osg_decode_publication(const cJSON *p, const char *expected, osg_crypto crypto, osg_image *image, uint16_t width, uint16_t height) {
    const char *canonical = osg_string(p, "canonicalDocument"), *hash = osg_string(p, "publicationHash"); char actual[72];
    if (!image || !crypto.sha256 || !canonical || strlen(canonical) > OSG_DOCUMENT_BYTES || !osg_hash_id(hash) ||
        !same(hash, expected) || !size(get(p, "assets"), 0, 0) || !crypto.sha256(canonical, strlen(canonical), actual) || strcmp(actual, hash)) return false;
    cJSON *doc = osg_json_parse(canonical, strlen(canonical)); if (!doc) return false;
    memset(image, 0, sizeof(*image));
    bool valid = false;
    const cJSON *ui = get(doc, "ui"), *presentation = get(doc, "presentation");
    const cJSON *pages = get(ui, "pages"), *controls = get(ui, "controls"), *feedback = get(ui, "feedbackWidgets");
    const cJSON *widgets = get(presentation, "widgets"), *viewports = get(presentation, "viewports");
    if (number(doc, "schemaVersion") != 2 || number(ui, "schemaVersion") != 1 || !osg_stable_id(osg_string(ui, "id")) ||
        !size(pages, 1, OSG_MAX_PAGES) || !size(widgets, 1, OSG_MAX_WIDGETS) || !size(controls, 0, OSG_MAX_WIDGETS) ||
        !size(feedback, 0, OSG_MAX_WIDGETS) || !size(get(ui, "assets"), 0, 64) || !size(get(presentation, "groups"), 0, 0) ||
        !size(viewports, (size_t)cJSON_GetArraySize(pages), (size_t)cJSON_GetArraySize(pages))) goto done;
    for(const cJSON *asset=get(ui,"assets")->child;asset;asset=asset->next){
        const char *digest=osg_string(asset,"sha256"),*mime=osg_string(asset,"mime");
        if(!osg_stable_id(osg_string(asset,"id")) || !digest || strlen(digest)!=64 || !mime || (!same(mime,"image/png")&&!same(mime,"image/jpeg")&&!same(mime,"image/webp")) || !isfinite(number(asset,"sizeBytes")) || number(asset,"sizeBytes")<1 || number(asset,"sizeBytes")>2097152)goto done;
        for(size_t i=0;i<64;i++)if(!((digest[i]>='0'&&digest[i]<='9')||(digest[i]>='a'&&digest[i]<='f')))goto done;
        for(const cJSON *before=get(ui,"assets")->child;before!=asset;before=before->next)if(same(osg_string(before,"id"),osg_string(asset,"id")))goto done;
    }
    image->panel = (osg_panel){save(image, osg_string(ui, "id")), save(image, hash), image->pages, image->widgets,
        (size_t)cJSON_GetArraySize(pages), (size_t)cJSON_GetArraySize(widgets)};
    size_t buttons = 0, outputs = 0;
    for (size_t i = 0; i < image->panel.page_count; i++) {
        const cJSON *page = cJSON_GetArrayItem(pages, (int)i), *viewport = NULL;
        for (const cJSON *v = viewports->child; v; v = v->next) if (same(osg_string(v, "pageId"), osg_string(page, "id"))) {
            if (viewport) goto done;
            viewport = v;
        }
        const double w = number(viewport, "width"), h = number(viewport, "height");
        if (!viewport || get(viewport, "backgroundAssetId") || !isfinite(w) || !isfinite(h) || w < 320 || h < 320 || w > 8192 || h > 8192 || floor(w) != w || floor(h) != h) goto done;
        image->pages[i] = (osg_page){save(image, osg_string(page, "id")), save(image, osg_string(page, "title")), (uint16_t)w, (uint16_t)h};
    }
    for (size_t i = 0; i < image->panel.widget_count; i++) {
        const cJSON *v = cJSON_GetArrayItem(widgets, (int)i), *config = get(v, "config"), *style = get(v, "style");
        osg_widget *w = &image->widgets[i];
        const char *kind = osg_string(v, "kind"), *control = NULL, *output = NULL;
        if (same(kind, "button")) w->kind = OSG_BUTTON;
        else if (same(kind, "indicator")) w->kind = OSG_INDICATOR;
        else if (same(kind, "text")) w->kind = OSG_TEXT;
        else if (same(kind, "numeric")) w->kind = OSG_NUMERIC;
        else if (same(kind, "gauge")) w->kind = OSG_GAUGE;
        else if (same(kind, "image")) w->kind = OSG_IMAGE;
        else goto done;
        const bool button = w->kind == OSG_BUTTON, bitmap = w->kind == OSG_IMAGE;
        if (get(v, "groupId") || !single_ref(get(v, "controlIds"), &control, button ? 1 : 0) ||
            !single_ref(get(v, "feedbackIds"), &output, button || bitmap ? 0 : 1)) goto done;
        const cJSON *primitive = find_id(button ? controls : feedback, button ? control : output);
        if (!bitmap && (!primitive || !same(osg_string(primitive, "kind"), button ? "button" : w->kind == OSG_INDICATOR ? "indicator" : w->kind == OSG_TEXT ? "text" : "gauge"))) goto done;
        if(bitmap){
            const cJSON *asset=find_id(get(ui,"assets"),osg_string(config,"assetId"));
            char digest[72];if(!asset)goto done;
            memcpy(digest,"sha256:",7);memcpy(digest+7,osg_string(asset,"sha256"),65);
            image->image_hashes[i]=save(image,digest);if(!image->image_hashes[i])goto done;
        }
        if (button) { if (!same(osg_string(config, "buttonMode"), "momentary") || number(config, "pulseMs") != 0 || number(config, "repeatMs") != 0 || get(config, "iconAssetId")) goto done; buttons++; }
        else if(!bitmap) outputs++;
        w->id = save(image, osg_string(v, "id")); w->reference = save(image, bitmap ? "" : button ? control : osg_string(primitive, "channelId"));
        w->output_id = save(image, output); w->label = save(image, osg_string(v, "label"));
        w->prefix = save(image, w->kind == OSG_TEXT ? osg_string(config, "prefix") : "");
        w->suffix = save(image, w->kind == OSG_TEXT ? osg_string(config, "suffix") : "");
        const bool numeric = w->kind == OSG_NUMERIC || w->kind == OSG_GAUGE;
        w->unit = save(image, numeric ? osg_string(config, "unit") : ""); w->min = numeric ? number(config, "min") : 0; w->max = numeric ? number(config, "max") : 1;
        const double precision = numeric ? number(config, "precision") : 0;
        if (!isfinite(precision) || precision < 0 || precision > 4 || floor(precision) != precision) goto done;
        w->precision = (uint8_t)precision;
        size_t page = 0; while (page < image->panel.page_count && !same(osg_string(v, "pageId"), image->pages[page].id)) page++;
        if (page == image->panel.page_count) goto done;
        w->page = (uint8_t)page;
        const cJSON *viewport = NULL;
        for (const cJSON *vp = viewports->child; vp; vp = vp->next) if (same(osg_string(vp, "pageId"), image->pages[page].id)) viewport = vp;
        const cJSON *insets = get(viewport, "safeInsets"), *bounds = get(v, "bounds");
        double left = 0, right = 0, top = 0, bottom = 0;
        if (insets) {
            left = number(insets, "left"); right = number(insets, "right"); top = number(insets, "top"); bottom = number(insets, "bottom");
            if (!isfinite(left) || !isfinite(right) || !isfinite(top) || !isfinite(bottom) || left < 0 || right < 0 || top < 0 || bottom < 0 || left > .25 || right > .25 || top > .25 || bottom > .25 || left + right >= .5 || top + bottom >= .5) goto done;
        }
        w->bounds = (osg_bounds){left + number(bounds, "x") * (1-left-right), top + number(bounds,"y") * (1-top-bottom), number(bounds,"width") * (1-left-right), number(bounds,"height") * (1-top-bottom)};
        if (!rgb(style,"foreground",0xdf3d,&w->foreground) || !rgb(style,"background",0x1126,&w->background) || !rgb(style,"accent",0x8718,&w->accent)) goto done;
        if (button) {
            const double scale = fmin((double)width/image->pages[page].width,(double)height/image->pages[page].height);
            if (floor(w->bounds.width*image->pages[page].width*scale) < 44 || floor(w->bounds.height*image->pages[page].height*scale) < 44) goto done;
        }
        for (size_t j = 0; j < i; j++) if (!button && !bitmap && image->widgets[j].kind != OSG_BUTTON && image->widgets[j].kind != OSG_IMAGE && same(w->output_id,image->widgets[j].output_id)) goto done;
    }
    if (buttons != (size_t)cJSON_GetArraySize(controls) || outputs != (size_t)cJSON_GetArraySize(feedback)) goto done;
    valid = osg_panel_valid(&image->panel);
done:
    cJSON_Delete(doc); return valid;
}
bool osg_open_identity(const cJSON *v, const char *host, const char *companion, const char *instance, const char *publication, osg_live_session *s) {
    uint64_t generation;
    if (!cJSON_IsTrue(get(v,"ok")) || !same(osg_string(v,"type"),"session-open") || !same(osg_string(v,"hostId"),host) ||
        !same(osg_string(v,"companionId"),companion) || !same(osg_string(v,"controllerInstanceId"),instance) ||
        !same(osg_string(v,"publicationHash"),publication) || !osg_stable_id(osg_string(v,"clientId")) ||
        !osg_stable_id(osg_string(v,"sessionId")) || !osg_counter(osg_string(v,"generation"),&generation) || !generation) return false;
    memset(s,0,sizeof(*s));
    return osg_copy(s->client,sizeof(s->client),osg_string(v,"clientId")) && osg_copy(s->session,sizeof(s->session),osg_string(v,"sessionId")) &&
        osg_copy(s->generation,sizeof(s->generation),osg_string(v,"generation")) && osg_copy(s->publication,sizeof(s->publication),publication) && osg_copy(s->instance,sizeof(s->instance),instance);
}
static bool feedback_envelope(const cJSON *frame, osg_live_session *session, osg_runtime *r, uint64_t now) {
    if(number(frame,"version")!=1 || !same(osg_string(frame,"type"),"feedback") ||
       !same(osg_string(frame,"clientId"),session->client) || !same(osg_string(frame,"sessionId"),session->session) ||
       !same(osg_string(frame,"generation"),session->generation) || !same(osg_string(frame,"publicationHash"),session->publication) ||
       !same(osg_string(frame,"controllerInstanceId"),session->instance)) return false;
    uint64_t sequence,generation;
    if(!osg_counter(osg_string(frame,"sequence"),&sequence) || !osg_counter(osg_string(frame,"bindingGeneration"),&generation))return false;
    const bool snapshot=same(osg_string(frame,"mode"),"snapshot");
    if(!snapshot&&!same(osg_string(frame,"mode"),"delta"))return false;
    if(session->feedback_envelope && generation<session->binding_generation)return false;
    uint8_t bindings[OSG_MAX_WIDGETS];size_t binding_count=0;
    if(snapshot){
        const cJSON *list=get(frame,"bindings");if(!size(list,0,OSG_MAX_WIDGETS))return false;
        for(const cJSON *b=list->child;b;b=b->next){
            size_t index=0;
            while(index<r->panel->widget_count){const osg_widget *w=&r->panel->widgets[index];
                if(w->kind!=OSG_BUTTON&&same(w->reference,osg_string(b,"channelId"))&&same(w->output_id,osg_string(b,"outputId")))break;
                index++;
            }
            if(index==r->panel->widget_count)return false;
            for(size_t j=0;j<binding_count;j++)if(same(r->panel->widgets[bindings[j]].reference,r->panel->widgets[index].reference))return false;
            bindings[binding_count++]=(uint8_t)index;
        }
    }else{
        if(get(frame,"bindings") || !session->feedback_envelope || generation!=session->binding_generation)return false;
        binding_count=session->binding_count;memcpy(bindings,session->feedback_bindings,binding_count);
    }
    const cJSON *values=get(frame,"values");if(!size(values,0,binding_count)||(snapshot&&(size_t)cJSON_GetArraySize(values)!=binding_count))return false;
    osg_runtime staged=*r;if(snapshot)memset(staged.samples,0,sizeof(staged.samples));
    bool seen[OSG_MAX_WIDGETS]={false};
    for(const cJSON *item=values->child;item;item=item->next){
        size_t binding=0;
        while(binding<binding_count){const osg_widget *w=&r->panel->widgets[bindings[binding]];
            if(same(w->reference,osg_string(item,"channelId"))&&same(w->output_id,osg_string(item,"outputId")))break;
            binding++;
        }
        if(binding==binding_count||seen[binding])return false;
        seen[binding]=true;const size_t index=bindings[binding];const osg_widget *w=&r->panel->widgets[index];
        const char *unit=osg_string(item,"unit"),*source=osg_string(item,"sourceKind"),*provenance=osg_string(item,"provenance"),*status=osg_string(item,"status");
        const double age=number(item,"sourceAgeMs");
        if(!unit||!*unit||strlen(unit)>32||!isfinite(age)||age<0||age>86400000||floor(age)!=age||
           (!same(source,"host")&&!same(source,"simulator")&&!same(source,"script")))return false;
        const char *provenances[]={"requested","local","host-accepted","host-submitted","game-confirmed"};
        int prov=0;while(prov<5&&!same(provenance,provenances[prov]))prov++;
        if(prov==5||(prov==4&&!same(source,"simulator")))return false;
        const bool numeric=w->kind==OSG_NUMERIC||w->kind==OSG_GAUGE;const cJSON *range=get(item,"range");
        if(numeric){if(!same(unit,*w->unit?w->unit:"ratio")||number(range,"min")!=w->min||number(range,"max")!=w->max)return false;}
        else if(range)return false;
        const cJSON *value=get(item,"value");const bool has_value=value&&!cJSON_IsNull(value);
        osg_sample sample={.available=has_value,.provenance=(osg_provenance)prov,.source_age_ms=(uint32_t)age,.received_ms=now};
        if(same(status,"fresh")){if(!has_value)return false;}
        else if(same(status,"unknown")){if(has_value)return false;sample.source_status=OSG_FEEDBACK_UNKNOWN;}
        else if(same(status,"disconnected")){if(has_value)return false;sample.source_status=OSG_FEEDBACK_DISCONNECTED;}
        else if(same(status,"stale"))sample.source_status=OSG_FEEDBACK_STALE;
        else return false;
        if(has_value){
            if(cJSON_IsBool(value)){sample.kind=OSG_BOOL;sample.boolean=cJSON_IsTrue(value);}
            else if(cJSON_IsNumber(value)){sample.kind=OSG_NUMBER;sample.number=value->valuedouble;}
            else if(cJSON_IsString(value)){sample.kind=OSG_STRING;if(!osg_copy(sample.text,sizeof(sample.text),value->valuestring))return false;}
            else return false;
            /* Validate every alias, but commit only the explicitly bound output. */
            if(!osg_feedback(&staged,w->reference,&sample,now))return false;
            sample=staged.samples[index];
            for(size_t j=0;j<r->panel->widget_count;j++)if(j!=index&&same(r->panel->widgets[j].reference,w->reference))staged.samples[j]=snapshot?(osg_sample){0}:r->samples[j];
        }
        staged.samples[index]=sample;
    }
    if(session->feedback_envelope&&generation==session->binding_generation){
        if(sequence<session->feedback_sequence)return false;
        if(sequence==session->feedback_sequence)return true;
    }
    memcpy(r->samples,staged.samples,sizeof(r->samples));
    memcpy(session->feedback_bindings,bindings,binding_count);session->binding_count=(uint8_t)binding_count;
    session->feedback_seen=true;session->feedback_envelope=true;session->feedback_sequence=sequence;session->binding_generation=generation;
    return true;
}
bool osg_ack(const cJSON *v, osg_live_session *session, const char *sequence, osg_runtime *r, uint64_t now) {
    if (!cJSON_IsTrue(get(v,"ok")) || !same(osg_string(v,"type"),"ack") || !same(osg_string(v,"clientId"),session->client) ||
        !same(osg_string(v,"sessionId"),session->session) || !same(osg_string(v,"generation"),session->generation) ||
        !same(osg_string(v,"sequence"),sequence) || !same(osg_string(v,"ackStage"),"source-accepted") || !cJSON_IsFalse(get(v,"submitted"))) return false;
    if(get(v,"publicationHash")&&!same(osg_string(v,"publicationHash"),session->publication))return false;
    const cJSON *values = get(v,"feedback"), *seq = get(v,"feedbackSequence");
    if(cJSON_IsObject(values))return feedback_envelope(values,session,r,now);
    if(session->feedback_envelope)return false;
    if (cJSON_IsNull(values) && cJSON_IsNull(seq)) return true;
    uint64_t feedback;
    if (!size(values,0,OSG_MAX_WIDGETS) || !osg_counter(cJSON_GetStringValue(seq),&feedback) ||
        (session->feedback_seen && feedback < session->feedback_sequence)) return false;
    if (session->feedback_seen && feedback == session->feedback_sequence) return true;
    osg_runtime copy = *r;
    for (const cJSON *item = values->child; item; item=item->next) {
        const char *channel = osg_string(item,"channelId"), *provenance = osg_string(item,"provenance"), *source = osg_string(item,"sourceKind");
        const cJSON *value = get(item,"value");
        const char *unit=osg_string(item,"unit");
        const double age = number(item,"sourceAgeMs");
        if (!osg_stable_id(channel) || !isfinite(age) || age < 0 || age > 86400000 || floor(age) != age ||
            (!same(source,"simulator") && !same(source,"host") && !same(source,"script")) || !unit || !*unit || strlen(unit)>32) return false;
        for (const cJSON *prev=values->child;prev!=item;prev=prev->next) if (same(channel,osg_string(prev,"channelId"))) return false;
        osg_sample sample = {.available=true,.source_age_ms=(uint32_t)age};
        const char *provenances[]={"requested","local","host-accepted","host-submitted","game-confirmed"};
        int index=0; while(index<5&&!same(provenance,provenances[index])) index++;
        if(index==5) return false;
        sample.provenance=(osg_provenance)index;
        if(cJSON_IsBool(value)){sample.kind=OSG_BOOL;sample.boolean=cJSON_IsTrue(value);}
        else if(cJSON_IsNumber(value)){sample.kind=OSG_NUMBER;sample.number=value->valuedouble;}
        else if(cJSON_IsString(value)){sample.kind=OSG_STRING;if(!osg_copy(sample.text,sizeof(sample.text),value->valuestring))return false;}
        else return false;
        const cJSON *range=get(item,"range");
        if(range){double min=number(range,"min"),max=number(range,"max");if(sample.kind!=OSG_NUMBER||!isfinite(min)||!isfinite(max)||min>=max||sample.number<min||sample.number>max)return false;}
        for(size_t i=0;i<r->panel->widget_count;i++){const osg_widget *w=&r->panel->widgets[i];if((w->kind==OSG_NUMERIC||w->kind==OSG_GAUGE)&&same(w->reference,channel)&&!same(w->unit,unit))return false;}
        if(!osg_feedback(&copy,channel,&sample,now))return false;
    }
    memcpy(r->samples,copy.samples,sizeof(r->samples));
    session->feedback_seen=true;session->feedback_sequence=feedback;return true;
}
