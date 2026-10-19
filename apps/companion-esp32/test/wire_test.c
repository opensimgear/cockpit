#include "wire.h"
#include "settings.h"
#include "panel.h"
#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <openssl/sha.h>
#include <openssl/evp.h>
static bool sha(const void *p,size_t n,char out[72]){unsigned char digest[32];SHA256(p,n,digest);memcpy(out,"sha256:",7);for(size_t i=0;i<32;i++)snprintf(out+7+i*2,3,"%02x",digest[i]);return true;}
static bool decode(const char *s,unsigned char *out,size_t capacity,size_t *n){size_t len=strlen(s);if(!len||len%4)return false;unsigned char *tmp=malloc(len);assert(tmp);int count=EVP_DecodeBlock(tmp,(const unsigned char *)s,(int)len);if(count<0){free(tmp);return false;}if(s[len-1]=='=')count--;if(s[len-2]=='=')count--;if((size_t)count>capacity){free(tmp);return false;}memcpy(out,tmp,(size_t)count);free(tmp);*n=(size_t)count;return true;}
static osg_crypto crypto={sha,decode};
static char raw[OSG_REPLY_BYTES+1], transfer_bytes[OSG_REPLY_BYTES];
static osg_image image;
static cJSON *load(const char *path){FILE *f=fopen(path,"rb");assert(f);size_t n=fread(raw,1,sizeof(raw)-1,f);assert(!ferror(f));fclose(f);raw[n]=0;cJSON *v=osg_json_parse(raw,n);assert(v);return v;}
static cJSON *json(const char *s){cJSON *v=osg_json_parse(s,strlen(s));assert(v);return v;}
static void string(cJSON *v,const char *k,const char *s){assert(cJSON_ReplaceItemInObjectCaseSensitive(v,k,cJSON_CreateString(s)));}
static void num(cJSON *v,const char *k,double n){assert(cJSON_ReplaceItemInObjectCaseSensitive(v,k,cJSON_CreateNumber(n)));}
static void dump(cJSON *v,char *out,size_t n){assert(cJSON_PrintPreallocated(v,out,(int)n,false));}
static void publication_tests(cJSON *p){
    const char *hash=osg_string(p,"publicationHash");
    assert(osg_decode_publication(p,hash,crypto,&image,800,480));assert(image.panel.page_count==2&&image.panel.widget_count==5);
    for(size_t i=0;i<5;i++){assert(!strcmp(image.widgets[i].id,osg_demo_widgets[i].id));assert(!strcmp(image.widgets[i].reference,osg_demo_widgets[i].reference));assert(fabs(image.widgets[i].bounds.x-osg_demo_widgets[i].bounds.x)<1e-12);assert(image.widgets[i].foreground==osg_demo_widgets[i].foreground);}
    assert(!osg_decode_publication(p,"sha256:0000000000000000000000000000000000000000000000000000000000000000",crypto,&image,800,480));
    assert(!osg_decode_publication(p,hash,crypto,&image,64,64));
    const char *canonical=osg_string(p,"canonicalDocument");cJSON *doc=json(canonical);char document[OSG_DOCUMENT_BYTES+1],new_hash[72];
    for(int mutation=0;mutation<22;mutation++){
        cJSON *copy=cJSON_Duplicate(doc,true),*ui=cJSON_GetObjectItemCaseSensitive(copy,"ui"),*pr=cJSON_GetObjectItemCaseSensitive(copy,"presentation");
        cJSON *widgets=cJSON_GetObjectItemCaseSensitive(pr,"widgets"),*w=cJSON_GetArrayItem(widgets,0),*cfg=cJSON_GetObjectItemCaseSensitive(w,"config");
        cJSON *bounds=cJSON_GetObjectItemCaseSensitive(w,"bounds"),*vp=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(pr,"viewports"),0);
        switch(mutation){
            case 0:num(copy,"schemaVersion",99);break;
            case 1:num(ui,"schemaVersion",2);break;
            case 2:string(w,"kind","xy");break;
            case 3:string(cfg,"buttonMode","toggle");break;
            case 4:num(cfg,"pulseMs",1);break;
            case 5:cJSON_AddStringToObject(cfg,"iconAssetId","icon");break;
            case 6:string(w,"pageId","missing");break;
            case 7:num(bounds,"x",-1);break;
            case 8:num(vp,"width",0);break;
            case 9:cJSON_AddStringToObject(vp,"backgroundAssetId","asset");break;
            case 10:cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(pr,"groups"),json("{\"id\":\"g\"}"));break;
            case 11:cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(ui,"assets"),json("{\"id\":\"asset\"}"));break;
            case 12:string(cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(ui,"controls"),0),"kind","hat");break;
            case 13:string(w,"label","bad [glyph]");break;
            case 14:cJSON_AddStringToObject(w,"groupId","g");break;
            case 15:num(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(widgets,3),"config"),"precision",5);break;
            case 16:cJSON_ReplaceItemInObjectCaseSensitive(w,"controlIds",json("[\"missing\"]"));break;
            case 17:cJSON_AddItemToObject(w,"style",json("{\"foreground\":\"#xx0000\"}"));break;
            case 18:cJSON_AddItemToObject(vp,"safeInsets",json("{\"top\":1,\"left\":0,\"right\":0,\"bottom\":0}"));break;
            case 19:num(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(widgets,3),"config"),"min",700);break;
            case 20:string(cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(ui,"feedbackWidgets"),0),"kind","text");break;
            case 21:cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(ui,"feedbackWidgets"),3);break;
        }
        dump(copy,document,sizeof(document));sha(document,strlen(document),new_hash);
        cJSON *bad=cJSON_Duplicate(p,true);string(bad,"canonicalDocument",document);string(bad,"publicationHash",new_hash);
        assert(!osg_decode_publication(bad,new_hash,crypto,&image,800,480));cJSON_Delete(bad);cJSON_Delete(copy);
    }
    for(int mutation=0;mutation<14;mutation++){
        cJSON *bad=cJSON_Duplicate(p,true),*copy=cJSON_Duplicate(doc,true);
        cJSON *pr=cJSON_GetObjectItemCaseSensitive(copy,"presentation"),*widgets=cJSON_GetObjectItemCaseSensitive(pr,"widgets"),*vp=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(pr,"viewports"),0);
        switch(mutation){
            case 0:string(bad,"canonicalDocument","not-json");break;
            case 1:string(bad,"publicationHash","invalid");break;
            case 2:cJSON_DeleteItemFromObjectCaseSensitive(bad,"assets");break;
            case 3:cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(bad,"assets"),cJSON_CreateObject());break;
            case 4:num(vp,"height",8193);break;
            case 5:num(vp,"width",800.5);break;
            case 6:cJSON_AddItemToObject(vp,"safeInsets",json("{\"top\":0,\"left\":0,\"right\":0,\"bottom\":0}"));cJSON_AddItemToObject(cJSON_GetArrayItem(widgets,1),"style",json("{\"foreground\":\"#AbC123\",\"background\":\"#ffffff\",\"accent\":\"#000000\"}"));break;
            case 7:cJSON_AddItemToObject(cJSON_GetArrayItem(widgets,1),"style",json("{\"foreground\":\"red\"}"));break;
            case 8:cJSON_ReplaceItemInObjectCaseSensitive(cJSON_GetArrayItem(widgets,1),"feedbackIds",json("[]"));break;
            case 9:string(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(widgets,2),"config"),"prefix","bad [font]");break;
            case 10:cJSON_ReplaceItemInObjectCaseSensitive(cJSON_GetArrayItem(widgets,1),"bounds",cJSON_Duplicate(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(widgets,0),"bounds"),true));break;
            case 11:cJSON_AddItemToArray(widgets,cJSON_Duplicate(cJSON_GetArrayItem(widgets,1),true));break;
            case 12:cJSON_ReplaceItemInObjectCaseSensitive(cJSON_GetArrayItem(widgets,3),"feedbackIds",json("[\"caution\"]"));break;
            case 13:num(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(widgets,3),"config"),"precision",1.5);break;
        }
        if(mutation>=4){dump(copy,document,sizeof(document));sha(document,strlen(document),new_hash);string(bad,"canonicalDocument",document);string(bad,"publicationHash",new_hash);}
        bool ok=osg_decode_publication(bad,osg_string(bad,"publicationHash"),crypto,&image,800,480);assert(ok==(mutation==6));cJSON_Delete(copy);cJSON_Delete(bad);
    }
    /* Display-only panels retain feedback widgets without requiring a button. */
    cJSON *copy=cJSON_Duplicate(doc,true),*ui=cJSON_GetObjectItemCaseSensitive(copy,"ui"),*pr=cJSON_GetObjectItemCaseSensitive(copy,"presentation");
    cJSON_ReplaceItemInObjectCaseSensitive(ui,"controls",json("[]"));
    cJSON_ReplaceItemInObjectCaseSensitive(copy,"bindings",json("[]"));
    cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(pr,"widgets"),0);
    cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(ui,"pages"),0),"items"),0);
    dump(copy,document,sizeof(document));sha(document,strlen(document),new_hash);
    cJSON *display=cJSON_Duplicate(p,true);string(display,"canonicalDocument",document);string(display,"publicationHash",new_hash);
    assert(osg_decode_publication(display,new_hash,crypto,&image,800,480));
    assert(image.panel.widget_count==4);
    for(size_t i=0;i<image.panel.widget_count;i++)assert(image.widgets[i].kind!=OSG_BUTTON);
    cJSON_Delete(display);cJSON_Delete(copy);
    copy=cJSON_Duplicate(doc,true);ui=cJSON_GetObjectItemCaseSensitive(copy,"ui");pr=cJSON_GetObjectItemCaseSensitive(copy,"presentation");
    cJSON *bitmap=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(pr,"widgets"),0);string(bitmap,"kind","image");cJSON_ReplaceItemInObjectCaseSensitive(bitmap,"config",json("{\"assetId\":\"picture\"}"));cJSON_ReplaceItemInObjectCaseSensitive(bitmap,"controlIds",json("[]"));cJSON_ReplaceItemInObjectCaseSensitive(ui,"controls",json("[]"));
    cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(ui,"assets"),json("{\"id\":\"picture\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\",\"mime\":\"image/png\",\"sizeBytes\":100}"));
    dump(copy,document,sizeof(document));sha(document,strlen(document),new_hash);display=cJSON_Duplicate(p,true);string(display,"canonicalDocument",document);string(display,"publicationHash",new_hash);
    assert(osg_decode_publication(display,new_hash,crypto,&image,800,480));assert(image.widgets[0].kind==OSG_IMAGE&&!image.widgets[0].pixels);cJSON_Delete(display);cJSON_Delete(copy);
    cJSON_Delete(doc);assert(osg_decode_publication(p,hash,crypto,&image,800,480));
}
static void transfer_tests(cJSON *p){
    char encoded[11000], frame[OSG_FRAME_BYTES+1],payload[OSG_REPLY_BYTES],hash[72];dump(p,payload,sizeof(payload));sha(payload,strlen(payload),hash);
    osg_transfer t={.bytes=transfer_bytes,.capacity=sizeof(transfer_bytes)};cJSON *out=NULL;
    snprintf(frame,sizeof(frame),"{\"type\":\"transfer-start\",\"id\":\"transfer\",\"bytes\":%zu,\"sha256\":\"%s\"}",strlen(payload),hash);
    assert(osg_reply_feed(&t,frame,strlen(frame),crypto,&out)==0);
    for(size_t offset=0,index=0;offset<strlen(payload);index++){
        size_t n=strlen(payload)-offset;if(n>512)n=512;
        EVP_EncodeBlock((unsigned char *)encoded,(const unsigned char *)payload+offset,(int)n);
        snprintf(frame,sizeof(frame),"{\"type\":\"transfer-chunk\",\"id\":\"transfer\",\"index\":%zu,\"base64\":\"%s\"}",index,encoded);
        assert(osg_reply_feed(&t,frame,strlen(frame),crypto,&out)==0);offset+=n;
    }
    const char *end="{\"type\":\"transfer-end\",\"id\":\"transfer\"}";
    assert(osg_reply_feed(&t,end,strlen(end),crypto,&out)==1);assert(!strcmp(osg_string(out,"publicationHash"),osg_string(p,"publicationHash")));cJSON_Delete(out);
    const char *bad[]={"{} trailing","[]","{\"type\":\"transfer-chunk\"}","{\"type\":\"transfer-end\"}","{\"type\":\"transfer-start\",\"id\":\"x\",\"bytes\":99999,\"sha256\":\"x\"}"};
    for(size_t i=0;i<sizeof(bad)/sizeof(bad[0]);i++)assert(osg_reply_feed(&t,bad[i],strlen(bad[i]),crypto,&out)==-1);
    snprintf(frame,sizeof(frame),"{\"type\":\"transfer-start\",\"id\":\"x\",\"bytes\":2,\"sha256\":\"%s\"}",hash);
    const char *failures[]={"{}","{\"type\":\"transfer-chunk\",\"id\":\"x\",\"index\":1,\"base64\":\"e30=\"}","{\"type\":\"transfer-chunk\",\"id\":\"x\",\"index\":0,\"base64\":\"%%%%\"}","{\"type\":\"transfer-end\",\"id\":\"x\"}"};
    for(size_t i=0;i<sizeof(failures)/sizeof(failures[0]);i++){t.active=false;assert(osg_reply_feed(&t,frame,strlen(frame),crypto,&out)==0);assert(osg_reply_feed(&t,failures[i],strlen(failures[i]),crypto,&out)==-1);}
    assert(osg_reply_feed(&t,"{\"ok\":true}",11,crypto,&out)==1);cJSON_Delete(out);
}
static void fill(void *p,int x,int y,int w,int h,uint16_t c){(void)p;(void)x;(void)y;(void)w;(void)h;(void)c;}
static void text(void *p,int x,int y,int w,int h,const char *s,uint16_t c){(void)s;fill(p,x,y,w,h,c);}
static void ack_tests(void){
    char open[1024];snprintf(open,sizeof(open),"{\"ok\":true,\"type\":\"session-open\",\"hostId\":\"host\",\"companionId\":\"screen\",\"controllerInstanceId\":\"instance\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"publicationHash\":\"%s\"}",osg_demo_panel.publication_hash);
    cJSON *v=json(open);osg_live_session session;
    assert(osg_open_identity(v,"host","screen","instance",osg_demo_panel.publication_hash,&session));
    assert(!osg_open_identity(v,"wrong","screen","instance",osg_demo_panel.publication_hash,&session));cJSON_Delete(v);
    osg_runtime r;assert(osg_init(&r,&osg_demo_panel,(osg_display){NULL,800,480,fill,text,NULL},NULL,NULL));osg_connected(&r,true);
    v=json("{\"ok\":true,\"type\":\"ack\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"sequence\":\"1\",\"ackStage\":\"source-accepted\",\"submitted\":false,\"feedbackSequence\":\"1\",\"feedback\":[{\"channelId\":\"speed-channel\",\"value\":125.5,\"provenance\":\"game-confirmed\",\"sourceKind\":\"simulator\",\"sourceAgeMs\":20,\"unit\":\"kt\"}]}" );
    assert(osg_ack(v,&session,"1",&r,100));assert(r.samples[3].number==125.5&&!r.armed);assert(osg_ack(v,&session,"1",&r,500));assert(r.samples[3].received_ms==100);
    assert(!osg_ack(v,&session,"2",&r,500));string(v,"sessionId","old");assert(!osg_ack(v,&session,"1",&r,500));string(v,"sessionId","session");
    string(v,"feedbackSequence","0");assert(!osg_ack(v,&session,"1",&r,500));string(v,"feedbackSequence","2");
    cJSON *sample=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(v,"feedback"),0);num(sample,"value",999);assert(!osg_ack(v,&session,"1",&r,500));assert(r.samples[3].number==125.5);
    num(sample,"value",100);string(sample,"provenance","forged");assert(!osg_ack(v,&session,"1",&r,500));string(sample,"provenance","local");
    num(sample,"sourceAgeMs",86400001);assert(!osg_ack(v,&session,"1",&r,500));num(sample,"sourceAgeMs",10);
    assert(osg_ack(v,&session,"1",&r,500));
    string(v,"feedbackSequence","3");cJSON_AddItemToObject(sample,"range",json("{\"min\":0,\"max\":600}"));assert(osg_ack(v,&session,"1",&r,600));
    string(v,"feedbackSequence","4");string(sample,"unit","mph");assert(!osg_ack(v,&session,"1",&r,700));string(sample,"unit","kt");
    num(cJSON_GetObjectItemCaseSensitive(sample,"range"),"min",700);assert(!osg_ack(v,&session,"1",&r,700));cJSON_DeleteItemFromObjectCaseSensitive(sample,"range");
    string(sample,"sourceKind","forged");assert(!osg_ack(v,&session,"1",&r,700));string(sample,"sourceKind","host");
    string(sample,"channelId","caution-channel");cJSON_ReplaceItemInObjectCaseSensitive(sample,"value",cJSON_CreateBool(true));assert(osg_ack(v,&session,"1",&r,700));
    string(v,"feedbackSequence","5");string(sample,"channelId","radio-channel");string(sample,"value","123.450");string(sample,"sourceKind","script");string(sample,"provenance","requested");assert(osg_ack(v,&session,"1",&r,800));
    string(v,"feedbackSequence","6");cJSON_AddItemToArray(cJSON_GetObjectItemCaseSensitive(v,"feedback"),cJSON_Duplicate(sample,true));assert(!osg_ack(v,&session,"1",&r,900));cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(v,"feedback"),1);
    cJSON_ReplaceItemInObjectCaseSensitive(sample,"value",cJSON_CreateNull());assert(!osg_ack(v,&session,"1",&r,900));
    cJSON_Delete(v);
    v=json("{\"ok\":true,\"type\":\"ack\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"sequence\":\"1\",\"ackStage\":\"source-accepted\",\"submitted\":false,\"feedbackSequence\":null,\"feedback\":null}");assert(osg_ack(v,&session,"1",&r,500));cJSON_Delete(v);
}
static cJSON *modern_ack(void){
    cJSON *v=json("{\"ok\":true,\"type\":\"ack\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"sequence\":\"1\",\"ackStage\":\"source-accepted\",\"submitted\":false,\"feedback\":{\"version\":1,\"type\":\"feedback\",\"clientId\":\"client\",\"sessionId\":\"session\",\"generation\":\"1\",\"controllerInstanceId\":\"instance\",\"sequence\":\"1\",\"bindingGeneration\":\"1\",\"mode\":\"snapshot\",\"bindings\":[{\"channelId\":\"speed-channel\",\"outputId\":\"speed\"}],\"values\":[{\"channelId\":\"speed-channel\",\"outputId\":\"speed\",\"status\":\"fresh\",\"value\":125.5,\"provenance\":\"game-confirmed\",\"sourceKind\":\"simulator\",\"sourceAgeMs\":0,\"unit\":\"kt\",\"range\":{\"min\":0,\"max\":600}}]}}" );
    cJSON_AddStringToObject(v,"publicationHash",osg_demo_panel.publication_hash);
    cJSON_AddStringToObject(cJSON_GetObjectItemCaseSensitive(v,"feedback"),"publicationHash",osg_demo_panel.publication_hash);
    return v;
}
static void modern_feedback_tests(void){
    osg_live_session session={.client="client",.session="session",.generation="1",.instance="instance"};osg_copy(session.publication,sizeof(session.publication),osg_demo_panel.publication_hash);
    osg_runtime r;assert(osg_init(&r,&osg_demo_panel,(osg_display){NULL,800,480,fill,text,NULL},NULL,NULL));osg_connected(&r,true);
    cJSON *base=modern_ack();assert(osg_ack(base,&session,"1",&r,100));assert(session.feedback_envelope&&r.samples[3].number==125.5);
    assert(osg_ack(base,&session,"1",&r,700));assert(r.samples[3].received_ms==100);
    for(int mutation=0;mutation<30;mutation++){
        cJSON *v=cJSON_Duplicate(base,true),*frame=cJSON_GetObjectItemCaseSensitive(v,"feedback"),*list=cJSON_GetObjectItemCaseSensitive(frame,"values"),*sample=cJSON_GetArrayItem(list,0),*bindings=cJSON_GetObjectItemCaseSensitive(frame,"bindings");
        string(frame,"sequence","2");
        switch(mutation){
            case 0:num(frame,"version",2);break;
            case 1:string(frame,"type","other");break;
            case 2:string(frame,"clientId","other");break;
            case 3:string(frame,"sessionId","old");break;
            case 4:string(frame,"generation","2");break;
            case 5:string(frame,"publicationHash","wrong");break;
            case 6:string(frame,"controllerInstanceId","wrong");break;
            case 7:string(frame,"sequence","01");break;
            case 8:string(frame,"bindingGeneration","bad");break;
            case 9:string(frame,"mode","bad");break;
            case 10:string(frame,"bindingGeneration","0");break;
            case 11:cJSON_DeleteItemFromObjectCaseSensitive(frame,"bindings");break;
            case 12:string(cJSON_GetArrayItem(bindings,0),"outputId","missing");break;
            case 13:cJSON_AddItemToArray(bindings,cJSON_Duplicate(cJSON_GetArrayItem(bindings,0),true));break;
            case 14:cJSON_DeleteItemFromArray(list,0);break;
            case 15:string(sample,"outputId","missing");break;
            case 16:num(sample,"sourceAgeMs",-1);break;
            case 17:string(sample,"unit","mph");break;
            case 18:string(sample,"sourceKind","fake");break;
            case 19:string(sample,"provenance","fake");break;
            case 20:string(sample,"sourceKind","host");break;
            case 21:num(cJSON_GetObjectItemCaseSensitive(sample,"range"),"min",1);break;
            case 22:cJSON_DeleteItemFromObjectCaseSensitive(sample,"value");break;
            case 23:string(sample,"status","unknown");break;
            case 24:string(sample,"status","disconnected");break;
            case 25:string(sample,"status","fake");break;
            case 26:cJSON_ReplaceItemInObjectCaseSensitive(sample,"value",cJSON_CreateArray());break;
            case 27:num(sample,"value",700);break;
            case 28:string(frame,"mode","delta");break;
            case 29:string(v,"publicationHash","wrong");break;
        }
        assert(!osg_ack(v,&session,"1",&r,800));assert(r.samples[3].number==125.5&&r.samples[3].received_ms==100);cJSON_Delete(v);
    }
    cJSON *delta=cJSON_Duplicate(base,true),*frame=cJSON_GetObjectItemCaseSensitive(delta,"feedback"),*sample=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(frame,"values"),0);
    string(frame,"mode","delta");string(frame,"sequence","2");cJSON_DeleteItemFromObjectCaseSensitive(frame,"bindings");num(sample,"value",128);
    assert(osg_ack(delta,&session,"1",&r,200));assert(r.samples[3].number==128&&r.samples[3].received_ms==200);
    string(frame,"sequence","3");cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(frame,"values"),0);assert(osg_ack(delta,&session,"1",&r,500));assert(r.samples[3].received_ms==200);
    string(frame,"sequence","2");assert(!osg_ack(delta,&session,"1",&r,600));cJSON_Delete(delta);
    for(int status=0;status<4;status++){
        cJSON *v=cJSON_Duplicate(base,true);frame=cJSON_GetObjectItemCaseSensitive(v,"feedback");sample=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(frame,"values"),0);
        string(frame,"bindingGeneration",status==0?"2":status==1?"3":status==2?"4":"5");string(frame,"sequence","1");
        string(sample,"status",status==0?"unknown":status==1?"disconnected":"stale");if(status!=2)cJSON_DeleteItemFromObjectCaseSensitive(sample,"value");
        assert(osg_ack(v,&session,"1",&r,700+(unsigned)status));assert(r.samples[3].available==(status==2));
        if(status==1)assert(r.samples[3].source_status==OSG_FEEDBACK_DISCONNECTED);
        cJSON_Delete(v);
    }
    cJSON *empty=cJSON_Duplicate(base,true);frame=cJSON_GetObjectItemCaseSensitive(empty,"feedback");string(frame,"bindingGeneration","6");
    cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(frame,"values"),0);cJSON_DeleteItemFromArray(cJSON_GetObjectItemCaseSensitive(frame,"bindings"),0);
    assert(osg_ack(empty,&session,"1",&r,800));assert(!r.samples[3].available);cJSON_Delete(empty);
    for(int kind=0;kind<2;kind++){
        cJSON *v=cJSON_Duplicate(base,true);frame=cJSON_GetObjectItemCaseSensitive(v,"feedback");string(frame,"bindingGeneration",kind?"8":"7");
        sample=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(frame,"values"),0);cJSON *binding=cJSON_GetArrayItem(cJSON_GetObjectItemCaseSensitive(frame,"bindings"),0);
        string(binding,"channelId",kind?"radio-channel":"caution-channel");string(binding,"outputId",kind?"radio":"caution");
        string(sample,"channelId",kind?"radio-channel":"caution-channel");string(sample,"outputId",kind?"radio":"caution");string(sample,"unit","state");
        string(sample,"provenance","requested");string(sample,"sourceKind","script");cJSON_DeleteItemFromObjectCaseSensitive(sample,"range");
        cJSON_ReplaceItemInObjectCaseSensitive(sample,"value",kind?cJSON_CreateString("123.450"):cJSON_CreateBool(true));
        assert(osg_ack(v,&session,"1",&r,900));assert(r.samples[kind?2:1].available);cJSON_Delete(v);
    }
    cJSON_Delete(base);
}
static void settings_tests(void){
    char data[4096];snprintf(data,sizeof(data),"{\"ssid\":\"wifi\",\"password\":\"wifi-password\",\"name\":\"Screen\",\"instanceId\":\"instance\",\"unixMs\":1800000000000,\"pairing\":{\"version\":1,\"endpoint\":\"wss://127.0.0.1:54437/companion\",\"hostId\":\"host\",\"caCertificate\":\"Y2E=\",\"caSpkiSha256\":\"%s\",\"secret\":\"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\",\"expiresAtMs\":60000}}",osg_demo_panel.publication_hash);
    cJSON *v=json(data);osg_settings settings;assert(osg_settings_parse(v,&settings));assert(settings.persist&&settings.schema==2&&!settings.token[0]);
    cJSON *pair=cJSON_GetObjectItemCaseSensitive(v,"pairing");
    num(pair,"expiresAtMs",-1);assert(!osg_settings_parse(v,&settings));num(pair,"expiresAtMs",1800000060000);
    string(pair,"endpoint","ws://host:80/companion");assert(!osg_settings_parse(v,&settings));string(pair,"endpoint","wss://host:54437/companion");
    string(pair,"secret","short");assert(!osg_settings_parse(v,&settings));cJSON_Delete(v);
    v=json(data);
    for(int mutation=0;mutation<23;mutation++){
        cJSON *copy=cJSON_Duplicate(v,true),*pp=cJSON_GetObjectItemCaseSensitive(copy,"pairing");
        switch(mutation){
            case 0:string(copy,"ssid","");break;
            case 1:cJSON_DeleteItemFromObjectCaseSensitive(copy,"password");break;
            case 2:string(copy,"name","");break;
            case 3:num(copy,"unixMs",0);break;
            case 4:num(copy,"unixMs",4102444800001.0);break;
            case 5:num(copy,"unixMs",1800000000000.5);break;
            case 6:num(pp,"expiresAtMs",9007199254740992.0);break;
            case 7:num(pp,"version",2);break;
            case 8:string(pp,"hostId","!");break;
            case 9:string(pp,"caSpkiSha256","bad");break;
            case 10:string(copy,"instanceId","bad id");break;
            case 11:cJSON_AddStringToObject(copy,"persist","yes");break;
            case 12:string(pp,"caCertificate","");break;
            case 13:cJSON_DeleteItemFromObjectCaseSensitive(pp,"expiresAtMs");break;
            case 14:cJSON_DeleteItemFromObjectCaseSensitive(copy,"name");break;
            case 15:cJSON_DeleteItemFromObjectCaseSensitive(copy,"ssid");break;
            case 16:string(copy,"password","0123456789012345678901234567890123456789012345678901234567890123456789");break;
            case 17:string(copy,"ssid","0123456789012345678901234567890123456789");break;
            case 18:cJSON_DeleteItemFromObjectCaseSensitive(pp,"version");break;
            case 19:num(pp,"expiresAtMs",1800000060000.5);break;
            case 20:string(copy,"name","012345678901234567890123456789012345678901234567890123456789012345678901234567890123456789");break;
            case 21:cJSON_DeleteItemFromObjectCaseSensitive(copy,"unixMs");break;
            case 22:cJSON_DeleteItemFromObjectCaseSensitive(pp,"caCertificate");break;
        }
        assert(!osg_settings_parse(copy,&settings));cJSON_Delete(copy);
    }
    cJSON_Delete(v);assert(!osg_settings_parse(NULL,&settings));assert(!osg_secret("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAB"));assert(!osg_secret("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA!"));
    uint64_t counter;assert(osg_counter("18446744073709551615",&counter)&&counter==UINT64_MAX);
    const char *bad[]={NULL,"","01","-1","18446744073709551616","+1","1e1"};for(size_t i=0;i<sizeof(bad)/sizeof(bad[0]);i++)assert(!osg_counter(bad[i],&counter));
    char host[254];int port;assert(osg_endpoint("wss://cockpit.local:54437/companion",host,&port));assert(port==54437&&!strcmp(host,"cockpit.local"));
    const char *urls[]={"ws://a:1/companion","wss://user@host:1/companion","wss://host:0/companion","wss://host:65536/companion","wss://host:1/companion?secret=x","wss://host/companion","wss://[::1]:1/companion"};for(size_t i=0;i<sizeof(urls)/sizeof(urls[0]);i++)assert(!osg_endpoint(urls[i],host,&port));
}
int main(int argc,char **argv){assert(argc==2);osg_json_init();assert(!osg_json_parse("{\"x\":1,\"x\":2}",13));assert(!osg_json_parse("{\"x\":1e999}",11));assert(!osg_json_parse("{} trailing",11));
    char host_check[254];int port_check;uint64_t count_check;
    assert(!osg_json_parse(NULL,1));assert(!osg_json_parse("",0));assert(!osg_json_parse("{}",OSG_REPLY_BYTES+1));
    assert(!osg_json_parse("{\"x\":\"\\u0000\"}",14));assert(!osg_endpoint(NULL,host_check,&port_check));
    assert(!osg_stable_id(NULL));assert(!osg_stable_id(""));assert(!osg_stable_id("!bad"));assert(!osg_hash_id(NULL));assert(!osg_hash_id("sha256:zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz"));
    assert(osg_counter("0",&count_check));
    char small[2];assert(!osg_copy(small,sizeof(small),NULL));assert(!osg_copy(small,sizeof(small),"long"));
    const char *badurls[]={"wss://:1/companion","wss://host:x/companion","wss://host:1/path","wss://host:123456/companion","wss://host:1","wss://host:/companion"};for(size_t i=0;i<sizeof(badurls)/sizeof(badurls[0]);i++)assert(!osg_endpoint(badurls[i],host_check,&port_check));
    cJSON *p=load(argv[1]);publication_tests(p);transfer_tests(p);ack_tests();modern_feedback_tests();settings_tests();cJSON_Delete(p);assert(osg_json_allocated()==0);
    char *large=malloc(OSG_JSON_HEAP_BYTES+1);assert(large);memset(large,'a',OSG_JSON_HEAP_BYTES);large[OSG_JSON_HEAP_BYTES]=0;assert(!cJSON_CreateString(large));free(large);assert(osg_json_allocated()==0);
    puts("Publication identity, bounded transfer, feedback, provisioning and allocation checks passed");return 0;}
