#include "../boards/sunton-3248s035r/board.c"
#include <assert.h>
#include <stdio.h>
static bool fail_spi,fail_lock;
static int irq=1,dc=1,z1=1000,z2=2000,raw_x=2050,raw_y=2050;
static unsigned transmits,pixel_bytes,devices;
static uint8_t last[512],cmd;
int gpio_config(const gpio_config_t *c){assert(c->pin_bit_mask);return ESP_OK;}
int gpio_set_level(int pin,int value){if(pin==2)dc=value;return ESP_OK;}
int gpio_get_level(int pin){assert(pin==36);return irq;}
SemaphoreHandle_t xSemaphoreCreateMutex(void){return (void *)1;}
int xSemaphoreTake(SemaphoreHandle_t h,unsigned ms){assert(h&&ms==100);return !fail_lock;}
int xSemaphoreGive(SemaphoreHandle_t h){assert(h);return 1;}
void vTaskDelay(unsigned ms){assert(ms==150||ms==120);}
int spi_bus_initialize(int host,const spi_bus_config_t *c,int dma){
 assert(host==SPI2_HOST&&dma==SPI_DMA_DISABLED&&c->mosi_io_num==13&&c->miso_io_num==12&&c->sclk_io_num==14&&c->max_transfer_sz==64);return ESP_OK;
}
int spi_bus_add_device(int host,const spi_device_interface_config_t *c,spi_device_handle_t *out){
 assert(host==SPI2_HOST&&c->queue_size==1);
 if(c->spics_io_num==15){assert(c->clock_speed_hz==24000000);*out=(void *)1;}
 else{assert(c->spics_io_num==33&&c->clock_speed_hz==2000000);*out=(void *)2;}
 devices++;return ESP_OK;
}
int spi_device_polling_transmit(spi_device_handle_t device,spi_transaction_t *t){
 transmits++;if(fail_spi)return -1;
 assert(t->length%8==0&&t->length<=512);
 const uint8_t *tx=t->tx_buffer;
 if(device==(void *)1){
  if(!dc){assert(t->length==8);cmd=tx[0];}
  else if(cmd==0x2c){pixel_bytes=(unsigned)t->length/8;memcpy(last,tx,pixel_bytes);}
 }else{
  assert(t->length==24);int value=tx[0]==0xb0?z1:tx[0]==0xc0?z2:tx[0]==0xd0?raw_x:raw_y;
  uint8_t *rx=t->rx_buffer;rx[0]=0;rx[1]=(uint8_t)(value>>5);rx[2]=(uint8_t)(value<<3);
 }
 return ESP_OK;
}
int main(void){
 osg_board board;assert(osg_board_init(&board));assert(devices==2&&board.width==480&&board.height==320&&board.present==NULL);
 const uint16_t rgb[]={0xf800,0x07e0,0x001f};
 assert(board.write(board.context,0,0,3,1,rgb));
 assert(pixel_bytes==6&&!memcmp(last,(uint8_t[]){0xf8,0,7,0xe0,0,0x1f},6));
 uint16_t stripe[100];for(unsigned i=0;i<100;i++)stripe[i]=0xffff;
 assert(board.write(board.context,0,0,100,1,stripe));assert(pixel_bytes==8);
 unsigned before=transmits;assert(!board.write(board.context,479,319,2,1,rgb));assert(transmits==before);
 bool down=true;int x=-1,y=-1;
 assert(board.touch(board.context,&down,&x,&y)&&!down);
 irq=0;assert(board.touch(board.context,&down,&x,&y)&&down&&x==239&&y==159);
 raw_x=200;raw_y=200;assert(board.touch(board.context,&down,&x,&y)&&down&&x==479&&y==0);
 raw_x=3900;raw_y=3900;assert(board.touch(board.context,&down,&x,&y)&&down&&x==0&&y==319);
 raw_x=0;assert(!board.touch(board.context,&down,&x,&y)&&!down);
 raw_x=2050;z1=0;z2=4095;assert(board.touch(board.context,&down,&x,&y)&&!down);
 z1=1000;z2=2000;fail_spi=true;assert(!board.touch(board.context,&down,&x,&y)&&!down);
 fail_spi=false;fail_lock=true;assert(!board.touch(board.context,&down,&x,&y));assert(!board.write(board.context,0,0,1,1,rgb));
 fail_lock=false;fail_spi=true;assert(!board.write(board.context,0,0,1,1,rgb));fail_spi=false;assert(!board.write(board.context,0,0,1,1,rgb));
 assert(osg_sunton_point(200,3900,200,3900,200,3900,&x,&y)&&x==0&&y==0);
 assert(osg_sunton_point(3900,200,200,3900,200,3900,&x,&y)&&x==479&&y==319);
 assert(!osg_sunton_point(2050,2050,200,200,200,3900,&x,&y));
 puts("Sunton display byte order/bounds and touch rotation/release/fault tests passed");
}
