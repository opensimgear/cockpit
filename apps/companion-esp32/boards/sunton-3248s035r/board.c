/* Sunton ESP32-3248S035R: original ESP-IDF ST7796/XPT2046 adapter.
 * Shared SPI2 bus; CPU-order RGB565 consumed synchronously, no framebuffer. */
#include "board.h"
#include "touch.h"
#include "driver/gpio.h"
#include "driver/spi_master.h"
#include "esp_log.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "sdkconfig.h"
#include <string.h>
#if !CONFIG_IDF_TARGET_ESP32
#error "Sunton 3248S035R requires IDF_TARGET=esp32"
#endif
#define WIDTH 480
#define HEIGHT 320
typedef struct {spi_device_handle_t lcd,pen;SemaphoreHandle_t mutex;uint8_t pixels[64];bool failed;} sunton;
static sunton hardware;
static bool transfer(spi_device_handle_t device,const void *tx,void *rx,size_t bytes){
    spi_transaction_t t={.length=bytes*8,.tx_buffer=tx,.rx_buffer=rx};
    return spi_device_polling_transmit(device,&t)==ESP_OK;
}
static bool command(sunton *b,uint8_t cmd,const uint8_t *data,size_t bytes){
    gpio_set_level(2,0);if(!transfer(b->lcd,&cmd,NULL,1))return false;
    gpio_set_level(2,1);return !bytes||transfer(b->lcd,data,NULL,bytes);
}
/* ESP32 without DMA has a 64-byte SPI FIFO; split even a full raster span. */
static bool write_pixels(void *context,int x,int y,int w,int h,const uint16_t *pixels){
    sunton *b=context;
    if(b->failed||!pixels||x<0||y<0||w<=0||h<=0||w>WIDTH-x||h>HEIGHT-y)return false;
    if(xSemaphoreTake(b->mutex,pdMS_TO_TICKS(100))!=pdTRUE)return false;
    uint8_t col[]={(uint8_t)(x>>8),(uint8_t)x,(uint8_t)((x+w-1)>>8),(uint8_t)(x+w-1)};
    uint8_t row[]={(uint8_t)(y>>8),(uint8_t)y,(uint8_t)((y+h-1)>>8),(uint8_t)(y+h-1)};
    bool ok=command(b,0x2a,col,4)&&command(b,0x2b,row,4)&&command(b,0x2c,NULL,0);
    size_t left=(size_t)w*h;
    while(ok&&left){
        size_t n=left>32?32:left;
        for(size_t i=0;i<n;i++){b->pixels[i*2]=(uint8_t)(pixels[i]>>8);b->pixels[i*2+1]=(uint8_t)pixels[i];}
        ok=transfer(b->lcd,b->pixels,NULL,n*2);pixels+=n;left-=n;
    }
    if(!ok)b->failed=true;
    xSemaphoreGive(b->mutex);return ok;
}
static bool adc(sunton *b,uint8_t cmd,int *value){
    uint8_t tx[]={cmd,0,0},rx[3]={0};
    if(!transfer(b->pen,tx,rx,3))return false;
    *value=(((rx[1]<<8)|rx[2])>>3)&4095;return true;
}
static int median(int v[5]){
    for(int i=1;i<5;i++){int n=v[i],j=i;while(j>0&&v[j-1]>n){v[j]=v[j-1];j--;}v[j]=n;}return v[2];
}
static bool touch(void *context,bool *down,int *x,int *y){
    sunton *b=context;*down=false;
    if(b->failed||xSemaphoreTake(b->mutex,pdMS_TO_TICKS(100))!=pdTRUE)return false;
    bool ok=true;
    if(gpio_get_level(36)==0){
        int xs[5],ys[5],z1=0,z2=0,unused=0;
        ok=adc(b,0xb0,&z1)&&adc(b,0xc0,&z2);
        for(int i=0;ok&&i<5;i++)ok=adc(b,0xd0,&xs[i])&&adc(b,0x90,&ys[i]);
        bool off=adc(b,0x90,&unused);ok=ok&&off;
        if(ok&&gpio_get_level(36)==0&&z1+4095-z2>=600){
            ok=osg_sunton_point(median(xs),median(ys),CONFIG_OSG_SUNTON_X_MIN,CONFIG_OSG_SUNTON_X_MAX,CONFIG_OSG_SUNTON_Y_MIN,CONFIG_OSG_SUNTON_Y_MAX,x,y);
            *down=ok;
        }
    }
    xSemaphoreGive(b->mutex);return ok;
}
static bool init_lcd(sunton *b){
    if(!command(b,0x01,NULL,0))return false;
    vTaskDelay(pdMS_TO_TICKS(150));
    const uint8_t u1=0xc3,u2=0x96,madctl=0x28,format=0x55;
    if(!command(b,0xf0,&u1,1)||!command(b,0xf0,&u2,1)||!command(b,0x36,&madctl,1)||!command(b,0x3a,&format,1))return false;
    const uint8_t inv=1,display[]={0x80,0x02,0x3b},entry=0xc6,power=0x80,vcom=0x18,adjust[]={0x40,0x8a,0,0,0x29,0x19,0xa5,0x33};
    const uint8_t pos[]={0xf0,9,0x0b,6,4,0x15,0x2f,0x54,0x42,0x3c,0x17,0x14,0x18,0x1b};
    const uint8_t neg[]={0xe0,9,0x0b,6,4,3,0x2b,0x43,0x42,0x3b,0x16,0x14,0x17,0x1b};
    if(!command(b,0xb4,&inv,1)||!command(b,0xb6,display,3)||!command(b,0xb7,&entry,1)||!command(b,0xc1,&power,1)||
       !command(b,0xc5,&vcom,1)||!command(b,0xe8,adjust,8)||!command(b,0xe0,pos,14)||!command(b,0xe1,neg,14))return false;
    const uint8_t l1=0x3c,l2=0x69;
    if(!command(b,0xf0,&l1,1)||!command(b,0xf0,&l2,1)||!command(b,0x11,NULL,0))return false;
    vTaskDelay(pdMS_TO_TICKS(120));return command(b,0x29,NULL,0);
}
bool osg_board_init(osg_board *out){
    sunton *b=&hardware;memset(b,0,sizeof(*b));
    gpio_config_t outputs={.pin_bit_mask=(1ULL<<27)|(1ULL<<2)|(1ULL<<15)|(1ULL<<33)|(1ULL<<5),.mode=GPIO_MODE_OUTPUT};
    if(gpio_config(&outputs)!=ESP_OK)return false;
    gpio_set_level(27,0);gpio_set_level(15,1);gpio_set_level(33,1);gpio_set_level(5,1);
    gpio_config_t irq={.pin_bit_mask=1ULL<<36,.mode=GPIO_MODE_INPUT};
    if(gpio_config(&irq)!=ESP_OK)return false;
    b->mutex=xSemaphoreCreateMutex();if(!b->mutex)return false;
    spi_bus_config_t bus={.mosi_io_num=13,.miso_io_num=12,.sclk_io_num=14,.quadwp_io_num=-1,.quadhd_io_num=-1,.max_transfer_sz=64};
    if(spi_bus_initialize(SPI2_HOST,&bus,SPI_DMA_DISABLED)!=ESP_OK)return false;
    spi_device_interface_config_t lcd={.clock_speed_hz=24000000,.mode=0,.spics_io_num=15,.queue_size=1};
    spi_device_interface_config_t pen={.clock_speed_hz=2000000,.mode=0,.spics_io_num=33,.queue_size=1};
    if(spi_bus_add_device(SPI2_HOST,&lcd,&b->lcd)!=ESP_OK||spi_bus_add_device(SPI2_HOST,&pen,&b->pen)!=ESP_OK||!init_lcd(b))return false;
    uint16_t black[WIDTH]={0};
    for(int y=0;y<HEIGHT;y++)if(!write_pixels(b,0,y,WIDTH,1,black))return false;
    gpio_set_level(27,1);
    *out=(osg_board){.width=WIDTH,.height=HEIGHT,.context=b,.write=write_pixels,.touch=touch};
    ESP_LOGI("companion","Sunton ESP32-3248S035R: ST7796 480x320 landscape, XPT2046 shared SPI; verify calibration");return true;
}
