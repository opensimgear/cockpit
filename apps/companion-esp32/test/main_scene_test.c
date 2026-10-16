#define OSG_RENDER_ENGINE 1
#define app_main firmware_app_main
#include "../main/app_main.c"
#undef app_main
#include <assert.h>
#include <stdio.h>
static osg_engine mock_engine;static osg_runtime snapshot;
static int acquire_result=1;static unsigned tiles,starts,finishes,presents,chrome,updates,release_calls,last_slice,failures;
static bool acquired,release_hits,present_ok=true,chrome_fail,slice_fail;
static uint64_t clock_us;
int64_t esp_timer_get_time(void){clock_us+=100;return (int64_t)clock_us;}
void osg_diagnostics_record(osg_diag_metric m,uint64_t us,uint64_t bytes,bool success){(void)m;(void)us;(void)bytes;(void)success;}
bool osg_copy(char *out,size_t n,const char *s){assert(strlen(s)<n);strcpy(out,s);return true;}
int osg_network_scene_acquire(osg_runtime *out,osg_engine **engine){assert(!acquired);if(acquire_result==1){acquired=true;*out=snapshot;*engine=&mock_engine;}return acquire_result;}
void osg_network_scene_release(bool publish){assert(acquired);acquired=false;release_calls++;release_hits=publish;}
void osg_network_render(const osg_panel *panel,bool finished,bool success){assert(acquired&&panel==snapshot.panel);if(finished){finishes++;if(!success)failures++;}else starts++;}
const char *osg_engine_current_package(const osg_engine *e){assert(e==&mock_engine);return e->package_hash;}
bool osg_engine_update_from_runtime(osg_engine *e,const osg_runtime *r,uint64_t now){(void)now;assert(acquired&&r==&drawing);updates++;if(e->failed||e->scene.pending||e->scene.awaiting_present)return false;if(tiles)e->scene.pending=true;return true;}
unsigned osg_engine_render_slice(osg_engine *e,unsigned maximum,osg_write_pixels write,void *context){(void)write;(void)context;assert(acquired&&maximum==8&&e->scene.pending);last_slice=maximum;if(slice_fail){e->failed=true;e->scene.failed=true;e->scene.pending=false;return 0;}unsigned n=tiles<maximum?tiles:maximum;tiles-=n;if(!tiles){e->scene.pending=false;e->scene.awaiting_present=true;}return n;}
void osg_engine_presented(osg_engine *e,bool success){assert(acquired);e->scene.awaiting_present=false;if(success)e->scene.ready=true;else{e->failed=e->scene.failed=true;e->scene.pending=false;e->scene.ready=false;}}
void osg_raster_status(osg_raster *r,const osg_runtime *runtime){assert(acquired&&runtime==&drawing);chrome++;r->failed=chrome_fail;}
static bool present(void *context){(void)context;assert(acquired);presents++;return present_ok;}
static bool write(void *c,int x,int y,int w,int h,const uint16_t *p){(void)c;(void)x;(void)y;(void)w;(void)h;(void)p;return true;}
static void reset_scene(const char *hash){memset(&mock_engine,0,sizeof(mock_engine));strcpy(mock_engine.package_hash,hash);tiles=0;slice_fail=chrome_fail=false;present_ok=true;}
int main(void){
    static const osg_panel panel={0};snapshot.panel=&panel;osg_board board={.width=480,.height=320,.write=write,.present=present};uint64_t rendered=0;
    acquire_result=-1;assert(scene_tick(&board,0,&rendered)==-1&&!release_calls);acquire_result=0;assert(scene_tick(&board,0,&rendered)==0&&!release_calls);acquire_result=1;
    reset_scene("package-A");snapshot.connected=true;tiles=16;
    assert(scene_tick(&board,1,&rendered)==1&&last_slice==8&&starts==1&&!finishes&&chrome==1&&!release_hits);
    assert(scene_tick(&board,2,&rendered)==0&&starts==1&&finishes==1&&presents==1&&release_hits&&rendered==2);
    unsigned saved=presents;assert(scene_tick(&board,3,&rendered)==0&&presents==saved&&!release_hits);
    /* Input-health changes must present chrome even if content has no damage. */
    snapshot.arming=true;assert(scene_tick(&board,4,&rendered)==0&&presents==saved+1&&release_hits);snapshot.arming=false;snapshot.armed=true;assert(scene_tick(&board,5,&rendered)==0&&presents==saved+2);
    snapshot.connected=false;assert(scene_tick(&board,6,&rendered)==0&&presents==saved+3);
    /* A transfer may replace the sole scene between bounded slices. */
    tiles=16;assert(scene_tick(&board,7,&rendered)==1);reset_scene("package-B");tiles=1;assert(scene_tick(&board,8,&rendered)==0&&release_hits);
    present_ok=false;tiles=1;assert(scene_tick(&board,9,&rendered)==0&&mock_engine.failed&&failures==1&&release_hits);saved=finishes;assert(scene_tick(&board,10,&rendered)==0&&finishes==saved);
    reset_scene("package-B");chrome_fail=true;tiles=1;saved=presents;assert(scene_tick(&board,11,&rendered)==0&&mock_engine.failed&&presents==saved&&failures==2&&!raster.failed);saved=finishes;assert(scene_tick(&board,12,&rendered)==0&&finishes==saved);
    reset_scene("package-C");tiles=1;slice_fail=true;assert(scene_tick(&board,13,&rendered)==0&&mock_engine.failed&&failures==3&&release_hits);
    reset_scene("package-D");tiles=1;board.present=NULL;assert(scene_tick(&board,14,&rendered)==0&&mock_engine.scene.ready&&release_hits);
    reset_scene("package-E");mock_engine.failed=true;assert(scene_tick(&board,15,&rendered)==0&&failures==4&&release_hits);
    reset_scene("package-F");tiles=16;assert(scene_tick(&board,16,&rendered)==1);snapshot.arming=!snapshot.arming;assert(scene_tick(&board,17,&rendered)==0&&release_hits);
    assert(!acquired&&updates>=17);puts("Main scene slice ownership, chrome-only presentation, transfer boundary, and display failure checks passed");return 0;
}