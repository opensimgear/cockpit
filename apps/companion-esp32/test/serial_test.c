#include "serial.h"
#include "sdkconfig.h"
#include "driver/uart.h"
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#ifndef CONFIG_OSG_SERIAL_BAUD
#define CONFIG_OSG_SERIAL_BAUD 921600
#endif
static bool installed;
static int install_error,line_error,baud_error,threshold_error,installs,deletes,uses;
static uart_event_t queued[32];static unsigned head,count,receives;
int uart_set_baudrate(int port,unsigned rate){assert(port==0&&rate==CONFIG_OSG_SERIAL_BAUD);return baud_error;}
int uart_set_rx_full_threshold(int port,int threshold){assert(port==0&&threshold==64);return threshold_error;}
bool uart_is_driver_installed(int port){assert(port==0);return installed;}
int uart_driver_install(int port,int rx,int tx,int capacity,QueueHandle_t *events,int flags){
    assert(port==0&&rx==9216&&!tx&&capacity==16&&events&&!flags);installs++;if(!install_error)*events=(void *)1;return install_error;
}
int uart_driver_delete(int port){assert(port==0);deletes++;return 0;}
int uart_vfs_dev_port_set_rx_line_endings(int port,int mode){assert(port==0&&mode==0);return line_error;}
void uart_vfs_dev_use_driver(int port){assert(port==0);uses++;}
int xQueueReceive(QueueHandle_t queue,void *event,unsigned wait){assert(queue==(void *)1&&!wait);receives++;if(!count)return 0;*(uart_event_t *)event=queued[head++];count--;return pdTRUE;}
unsigned uxQueueSpacesAvailable(QueueHandle_t queue){assert(queue==(void *)1);return count>=16?0:16-count;}
int main(void){
    osg_serial_health sample;osg_serial_health_poll();osg_serial_health_snapshot(NULL);
#if CONFIG_ESP_CONSOLE_UART
    assert(osg_serial_start()&&uses==1&&installs==1&&!deletes);
    installed=true;assert(!osg_serial_start()&&installs==1);installed=false;
    install_error=-1;assert(!osg_serial_start()&&uses==1&&installs==2&&!deletes);install_error=0;
    line_error=-1;assert(!osg_serial_start()&&uses==1&&deletes==1);line_error=0;
    baud_error=-1;assert(!osg_serial_start()&&deletes==2);baud_error=0;
    threshold_error=-1;assert(!osg_serial_start()&&deletes==3);threshold_error=0;
    unsigned before=receives;osg_serial_health_poll();assert(receives==before);
    assert(osg_serial_start()&&uses==2&&installs==6);
    const uart_event_type_t types[]={UART_FIFO_OVF,UART_BUFFER_FULL,UART_FRAME_ERR,UART_PARITY_ERR,UART_BREAK};
    count=17;head=0;for(unsigned i=0;i<count;i++)queued[i].type=i<5?types[i]:UART_DATA;
    before=receives;osg_serial_health_poll();assert(receives-before==16&&count==1);osg_serial_health_snapshot(&sample);
    assert(sample.fifo_overflows==1&&sample.buffer_full==1&&sample.frame_errors==1&&sample.parity_errors==1&&sample.breaks==1&&sample.queue_full_observations==1);
    osg_serial_health_poll();assert(!count);
#else
    assert(osg_serial_start()&&!installs&&!uses&&!deletes);osg_serial_health_poll();osg_serial_health_snapshot(&sample);assert(!sample.fifo_overflows);
#endif
    for(unsigned i=0;i<OSG_SERIAL_FAILURE_COUNT;i++)osg_serial_health_failure((osg_serial_failure)i);
    osg_serial_health_failure(OSG_SERIAL_FAILURE_COUNT);osg_serial_health_snapshot(&sample);
    assert(sample.request_timeouts==1&&sample.invalid_json==1&&sample.reply_mismatch==1&&sample.deferred_full==1&&sample.invalid_reply==1&&sample.write_failures==1&&sample.frame_overflows==1);
    puts("UART RX64 threshold, bounded error events, startup cleanup and scalar failure observations passed");return 0;
}
