#include "input.h"
#include "panel.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
static osg_runtime runtime;
static osg_input input;
static void fill(void *p,int x,int y,int w,int h,uint16_t c){(void)p;(void)x;(void)y;(void)w;(void)h;(void)c;}
static void text(void *p,int x,int y,int w,int h,const char *s,uint16_t c){(void)s;fill(p,x,y,w,h,c);}
static void setup(void){
    memset(&input,0,sizeof(input));
    assert(osg_init(&runtime,&osg_demo_panel,(osg_display){NULL,800,480,fill,text},NULL,NULL));
    osg_connected(&runtime,true);osg_input_bind(&input,&runtime,true,100);
    osg_input_poll(&input,true,false,0,0,100);osg_input_render(&input,runtime.panel,false,true,100);osg_input_render(&input,runtime.panel,true,true,101);
}
static uint64_t arm(uint64_t now){
    uint64_t epoch;assert(osg_input_prepare(&input,now,&epoch));assert(runtime.arming&&!runtime.armed);
    assert(osg_input_commit(&input,epoch,now,now));assert(runtime.armed&&runtime.live_input&&!runtime.arming);return epoch;
}
int main(void){
    setup();uint64_t epoch=arm(102);osg_input_poll(&input,true,true,100,100,103);assert(input.count==1&&runtime.active==0);
    osg_input_poll(&input,true,true,100,100,104);assert(input.count==1);osg_input_poll(&input,true,false,100,100,105);assert(input.count==2);
    osg_input_edge edges[16];size_t n=0;uint64_t observed=0;
    assert(osg_input_peek(&input,edges,16,&n,&observed,106));assert(n==2&&observed==epoch&&edges[0].down&&!edges[1].down&&edges[0].sequence==1&&edges[1].sequence==2);
    assert(!osg_input_accept(&input,epoch+1,1));assert(!osg_input_accept(&input,epoch,3));assert(!osg_input_accept(&input,epoch,0));
    assert(osg_input_accept(&input,epoch,1));assert(input.count==1);assert(osg_input_accept(&input,epoch,1));assert(input.count==0);
    for(int i=0;i<90;i++){uint64_t now=110+(unsigned)i;osg_input_poll(&input,true,true,100,100,now);osg_input_poll(&input,true,false,0,0,now);assert(osg_input_peek(&input,edges,16,&n,&observed,now));assert(n==2);assert(osg_input_accept(&input,epoch,n));}
    /* A stationary held button stays active while polls and ACKs are healthy. */
    osg_input_poll(&input,true,true,100,100,210);assert(osg_input_accept(&input,epoch,1));
    for(uint64_t now=220;now<3000;now+=20){input.last_ack_ms=now;osg_input_poll(&input,true,true,100,100,now);assert(runtime.armed&&runtime.active==0&&input.count==0);}
    osg_input_poll(&input,true,false,0,0,3000);assert(input.count==1&&!input.queue[input.head].down);
    setup();arm(102);osg_input_poll(&input,true,true,100,100,103);osg_input_poll(&input,true,true,-1,-1,104);assert(input.mode==OSG_INPUT_BLOCKED&&!runtime.armed&&!input.count);
    osg_input_poll(&input,true,false,0,0,105);assert(!osg_input_prepare(&input,105,&epoch));
    setup();arm(102);osg_input_poll(&input,false,false,0,0,103);assert(input.fault==OSG_INPUT_TOUCH_FAILED&&!runtime.armed);
    setup();arm(102);assert(!osg_input_monitor(&input,700));assert(input.fault==OSG_INPUT_UI_EXPIRED);
    setup();arm(102);osg_input_poll(&input,true,false,0,0,500);osg_input_poll(&input,true,false,0,0,900);assert(!osg_input_monitor(&input,1102));assert(input.fault==OSG_INPUT_HOST_EXPIRED);
    setup();arm(102);osg_input_render(&input,runtime.panel,false,true,103);osg_input_poll(&input,true,false,0,0,500);osg_input_poll(&input,true,false,0,0,704);assert(input.fault==OSG_INPUT_DISPLAY_FAILED);
    setup();arm(102);osg_input_render(&input,runtime.panel,true,false,104);assert(input.fault==OSG_INPUT_DISPLAY_FAILED);
    setup();arm(102);osg_input_poll(&input,true,true,100,100,103);assert(osg_input_peek(&input,edges,16,&n,&epoch,352));assert(!osg_input_peek(&input,edges,16,&n,&epoch,353));assert(input.fault==OSG_INPUT_BACKLOG);
    setup();arm(102);for(unsigned i=0;i<32;i++){osg_input_poll(&input,true,true,100,100,103+i);osg_input_poll(&input,true,false,0,0,103+i);}
    assert(input.count==64);osg_input_poll(&input,true,true,100,100,136);assert(input.fault==OSG_INPUT_OVERFLOW&&!runtime.armed&&!input.count);
    setup();arm(102);assert(!runtime.sink(runtime.sink_context,"master",true,2));assert(input.fault==OSG_INPUT_SEQUENCE);
    setup();arm(102);assert(!runtime.sink(runtime.sink_context,"unknown",true,1));assert(input.fault==OSG_INPUT_SEQUENCE);
    setup();arm(102);input.last_edge=UINT64_MAX;assert(!runtime.sink(runtime.sink_context,"master",true,1));
    setup();assert(!runtime.sink(runtime.sink_context,"master",true,1));
    setup();assert(osg_input_prepare(&input,102,&epoch));osg_input_poll(&input,true,true,100,100,103);osg_input_poll(&input,true,false,0,0,104);assert(!osg_input_commit(&input,epoch,102,105));assert(!runtime.armed&&!input.count);
    setup();assert(osg_input_prepare(&input,102,&epoch));assert(!osg_input_commit(&input,epoch+1,102,103));assert(!osg_input_commit(&input,epoch,104,103));
    setup();osg_input_poll(&input,true,true,100,100,102);assert(!osg_input_prepare(&input,103,&epoch));osg_input_poll(&input,true,false,0,0,104);assert(osg_input_prepare(&input,105,&epoch));
    setup();input.touch_capable=false;assert(!osg_input_prepare(&input,102,&epoch));
    setup();input.touch_valid=false;assert(!osg_input_prepare(&input,102,&epoch));
    setup();input.display_ready=false;assert(!osg_input_prepare(&input,102,&epoch));
    setup();runtime.connected=false;assert(!osg_input_prepare(&input,102,&epoch));
    setup();assert(!osg_input_prepare(&input,700,&epoch));input.last_poll_ms=1100;assert(!osg_input_prepare(&input,1101,&epoch));
    setup();input.epoch=UINT64_MAX-1;assert(!osg_input_prepare(&input,102,&epoch));input.epoch=UINT64_MAX;osg_input_bind(&input,&runtime,true,103);assert(input.mode==OSG_INPUT_BLOCKED);
    setup();arm(102);osg_set_page(&runtime,1);assert(!osg_input_monitor(&input,103));
    setup();arm(102);assert(!osg_input_monitor(&input,90));
    setup();input.rendering=true;input.render_start_ms=0;input.last_poll_ms=700;input.last_ack_ms=700;assert(!osg_input_prepare(&input,700,&epoch));
    setup();arm(102);input.touch_valid=false;assert(!osg_input_monitor(&input,103));
    setup();assert(osg_input_prepare(&input,102,&epoch));runtime.arming=false;assert(!osg_input_monitor(&input,103));
    setup();assert(osg_input_prepare(&input,102,&epoch));runtime.pointer_down=true;assert(!osg_input_commit(&input,epoch,102,103));
    setup();osg_panel another=osg_demo_panel;osg_input_render(&input,&osg_demo_panel,false,true,102);
    assert(osg_init(&runtime,&another,runtime.display,NULL,NULL));osg_connected(&runtime,true);osg_input_bind(&input,&runtime,true,103);
    osg_input_poll(&input,true,false,0,0,104);osg_input_render(&input,&osg_demo_panel,true,true,105);assert(!osg_input_can_arm(&input,106));
    osg_input_render(&input,&another,true,true,107);assert(osg_input_can_arm(&input,108));
    assert(osg_input_prepare(&input,108,&epoch));assert(osg_input_commit(&input,epoch,108,108));assert(!osg_input_peek(&input,edges,16,&n,&epoch,800));
    puts("Live queue, baseline races, liveness guards, cancellation and fresh rearm checks passed");return 0;
}
