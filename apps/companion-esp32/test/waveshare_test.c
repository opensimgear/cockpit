#include "../boards/waveshare-lcd7/board.c"
#include <assert.h>
#include <stdarg.h>
#include <stdio.h>

static uint16_t frames[2][WIDTH*HEIGHT];
static unsigned operation, fail_at, bus_count, devices, panels, semaphores, waits, pending;
static uint64_t clock_ms;
static size_t psram_bytes=8*1024*1024;
static bool wait_failure, wrong_product, wrong_geometry;
static uint8_t status, point[8], outputs_seen[32];
static unsigned outputs_count;
static vsync_callback callback;
static void *callback_context;
static bool step(void) { return ++operation != fail_at; }
size_t esp_psram_get_size(void){return psram_bytes;}
int64_t esp_timer_get_time(void){return (int64_t)clock_ms*1000;}
void vTaskDelay(unsigned ms){clock_ms+=ms;}
SemaphoreHandle_t xSemaphoreCreateCounting(unsigned max,unsigned initial){assert(max==3&&!initial);if(!step())return NULL;semaphores++;return (void *)1;}
int xSemaphoreTake(SemaphoreHandle_t s,unsigned ticks){
    assert(s);if(!ticks){if(pending){pending--;return pdTRUE;}return pdFALSE;}
    assert(ticks==80);waits++;clock_ms+=ticks;
    return wait_failure?pdFALSE:pdTRUE;
}
int xSemaphoreGiveFromISR(SemaphoreHandle_t s,BaseType_t *wake){assert(s);if(pending<3)pending++;*wake=pdTRUE;return pdTRUE;}
void vSemaphoreDelete(SemaphoreHandle_t s){assert(s&&semaphores);semaphores--;}
esp_err_t gpio_config(const gpio_config_t *c){assert(c->pin_bit_mask==(1ULL<<4)&&c->mode==GPIO_MODE_OUTPUT);return step()?ESP_OK:-1;}
esp_err_t gpio_set_level(int pin,int level){assert(pin==4&&level==0);return step()?ESP_OK:-1;}
esp_err_t gpio_set_direction(int pin,int direction){assert(pin==4&&direction==GPIO_MODE_INPUT);return step()?ESP_OK:-1;}
esp_err_t gpio_reset_pin(int pin){assert(pin==4);return ESP_OK;}
esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *c,i2c_master_bus_handle_t *out){
    assert(c->sda_io_num==8&&c->scl_io_num==9&&c->i2c_port==0);
    if(!step())return -1;
    *out=(void *)1;bus_count++;return ESP_OK;
}
esp_err_t i2c_del_master_bus(i2c_master_bus_handle_t b){assert(b&&bus_count);bus_count--;return ESP_OK;}
esp_err_t i2c_master_bus_add_device(i2c_master_bus_handle_t b,const i2c_device_config_t *c,i2c_master_dev_handle_t *out){
    assert(b&&c->scl_speed_hz==400000);
    if(!step())return -1;
    *out=(void *)(uintptr_t)c->device_address;devices++;return ESP_OK;
}
esp_err_t i2c_master_bus_rm_device(i2c_master_dev_handle_t d){assert(d&&devices);devices--;return ESP_OK;}
esp_err_t i2c_master_transmit(i2c_master_dev_handle_t d,const void *data,size_t size,int timeout){
    assert(timeout==30);const uint8_t *bytes=data;
    if(!step())return -1;

    if(d==(void *)0x24){assert(size==1&&bytes[0]==1);}
    else if(d==(void *)0x38){assert(size==1&&outputs_count<32);outputs_seen[outputs_count++]=bytes[0];}
    else {assert(d==(void *)0x5d&&size==3&&bytes[0]==0x81&&bytes[1]==0x4e&&!bytes[2]);}
    return ESP_OK;
}
esp_err_t i2c_master_transmit_receive(i2c_master_dev_handle_t d,const void *reg,size_t reg_size,void *data,size_t size,int timeout){
    assert(d==(void *)0x5d&&reg_size==2&&timeout==30);
    if(!step())return -1;

    const uint8_t *r=reg;unsigned address=(unsigned)r[0]<<8|r[1];
    if(address==0x8140){assert(size==4);memcpy(data,wrong_product?"bad":"911",4);}
    else if(address==0x8048){uint8_t dimensions[]={0x20,3,0xe0,1};assert(size==4);if(wrong_geometry)dimensions[0]=0;memcpy(data,dimensions,4);}
    else if(address==0x814e){assert(size==1);*(uint8_t *)data=status;}
    else {assert(address==0x814f&&size==8);memcpy(data,point,8);}
    return ESP_OK;
}
esp_err_t esp_lcd_new_rgb_panel(const esp_lcd_rgb_panel_config_t *c,esp_lcd_panel_handle_t *out){
    const int pins[]={14,38,18,17,10,39,0,45,48,47,21,1,2,42,41,40};
    assert(c->timings.h_res==800&&c->timings.v_res==480&&c->timings.pclk_hz==16000000);
    assert(c->timings.hsync_pulse_width==4&&c->timings.vsync_pulse_width==4&&c->timings.hsync_back_porch==8&&c->timings.vsync_front_porch==8);
    assert(c->num_fbs==2&&c->bounce_buffer_size_px==8000&&c->dma_burst_size==64&&c->flags.fb_in_psram);
    assert(c->hsync_gpio_num==46&&c->vsync_gpio_num==3&&c->de_gpio_num==5&&c->pclk_gpio_num==7&&c->disp_gpio_num==-1);
    assert(!memcmp(pins,c->data_gpio_nums,sizeof(pins)));
    if(!step())return -1;
    *out=(void *)1;panels++;return ESP_OK;
}
esp_err_t esp_lcd_rgb_panel_get_frame_buffer(esp_lcd_panel_handle_t p,unsigned n,void **first,...){
    assert(p&&n==2);if(!step())return -1;

    *first=frames[0];va_list args;va_start(args,first);*va_arg(args,void **)=frames[1];va_end(args);return ESP_OK;
}
esp_err_t esp_lcd_rgb_panel_register_event_callbacks(esp_lcd_panel_handle_t p,const esp_lcd_rgb_panel_event_callbacks_t *cb,void *ctx){
    assert(p);if(!step())return -1;
    callback=cb->on_vsync;callback_context=ctx;return ESP_OK;
}
esp_err_t esp_lcd_panel_reset(esp_lcd_panel_handle_t p){assert(p);return step()?ESP_OK:-1;}
esp_err_t esp_lcd_panel_init(esp_lcd_panel_handle_t p){assert(p);return step()?ESP_OK:-1;}
esp_err_t esp_lcd_panel_del(esp_lcd_panel_handle_t p){assert(p&&panels);panels--;return ESP_OK;}
esp_err_t esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t p,int x,int y,int w,int h,const void *data){
    assert(p&&!x&&!y&&w==800&&h==480&&data==hardware.fb[hardware.back]);return step()?ESP_OK:-1;
}
static void empty(void){assert(!bus_count&&!devices&&!panels&&!semaphores);}
static osg_board ready(void){
    cleanup(&hardware);empty();operation=fail_at=outputs_count=pending=waits=0;wait_failure=wrong_product=wrong_geometry=false;
    psram_bytes=8*1024*1024;osg_board b;assert(osg_board_init(&b));assert(b.width==800&&b.height==480&&b.touch&&b.present&&b.write);
    assert(outputs_count==4&&outputs_seen[0]==0x30&&outputs_seen[1]==0x38&&outputs_seen[2]==0x3a&&outputs_seen[3]==0x3e);return b;
}
static bool touch(osg_board *b){bool down;int x,y;return b->touch(b->context,&down,&x,&y);}
static void report(uint8_t count,int x,int y,uint8_t id){status=0x80|count;memset(point,0,8);point[0]=id;point[1]=(uint8_t)x;point[2]=(uint8_t)(x>>8);point[3]=(uint8_t)y;point[4]=(uint8_t)(y>>8);}
int main(void){
    osg_board b=ready();unsigned init_steps=operation;osg_board other;assert(!osg_board_init(&other)&&!other.write);
    cleanup(&hardware);empty();
    for(unsigned n=1;n<=init_steps;n++){
        operation=outputs_count=0;fail_at=n;assert(!osg_board_init(&b)&&!b.write);empty();
    }
    fail_at=0;psram_bytes=4*1024*1024;assert(!osg_board_init(&b));empty();
    psram_bytes=8*1024*1024;wrong_product=true;assert(!osg_board_init(&b));empty();wrong_product=false;
    wrong_geometry=true;assert(!osg_board_init(&b));empty();wrong_geometry=false;
    b=ready();uint16_t pixels[]={1,2,3,4};assert(b.write(b.context,798,478,2,2,pixels));
    assert(frames[1][478*800+798]==1&&frames[1][479*800+799]==4&&!frames[0][479*800+799]);
    assert(!b.write(b.context,-1,0,1,1,pixels));assert(!b.write(b.context,0,-1,1,1,pixels));
    assert(!b.write(b.context,800,0,1,1,pixels));assert(!b.write(b.context,0,480,1,1,pixels));
    assert(!b.write(b.context,0,0,0,1,pixels));assert(!b.write(b.context,0,0,1,0,pixels));assert(!b.write(b.context,0,0,1,1,NULL));
    assert(callback(NULL,NULL,callback_context));assert(pending==1);
    unsigned back=hardware.back;assert(b.present(b.context)&&waits==3&&hardware.back!=back);
    assert(b.present(b.context)&&hardware.back==back);
    fail_at=operation+1;assert(!b.present(b.context)&&hardware.display_failed);assert(!b.present(b.context)&&!b.write(b.context,0,0,1,1,pixels));
    b=ready();wait_failure=true;back=hardware.back;assert(!b.present(b.context)&&hardware.back==back);wait_failure=false;
    b=ready();status=0;assert(!touch(&b)); /* Startup needs an observed release. */
    report(0,0,0,0);assert(touch(&b)&&!hardware.down);status=0;clock_ms+=10000;assert(touch(&b));
    report(1,799,479,2);bool down=false;int x=0,y=0;assert(b.touch(b.context,&down,&x,&y)&&down&&x==799&&y==479);
    status=0;clock_ms+=199;assert(touch(&b)&&hardware.down);clock_ms++;assert(!touch(&b));
    report(1,0,0,1);assert(touch(&b));report(1,0,0,2);assert(!touch(&b)); /* Finger substitution. */
    report(0,0,0,0);assert(touch(&b));report(2,0,0,0);assert(!touch(&b));
    report(1,800,0,0);assert(!touch(&b));report(1,0,480,0);assert(!touch(&b));
    status=0x90;assert(!touch(&b)); /* Touch-key/invalid status. */
    report(1,20,30,0);assert(touch(&b));clock_ms--;status=0;assert(!touch(&b));
    for(unsigned offset=1;offset<=3;offset++){report(1,10,10,0);fail_at=operation+offset;assert(!touch(&b));fail_at=0;}
    report(0,0,0,0);assert(touch(&b));
    cleanup(&hardware);empty();assert(!touch(&b)&&!present(&hardware)&&!write_pixels(&hardware,0,0,1,1,pixels));
    puts("Waveshare init, geometry, touch freshness, multitouch, frame fences and failure cleanup passed");
    return 0;
}
