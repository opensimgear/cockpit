#include "serial.h"
#include "sdkconfig.h"
#include "driver/uart.h"
#include "freertos/task.h"
#include <assert.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#ifndef CONFIG_OSG_SERIAL_BAUD
#define CONFIG_OSG_SERIAL_BAUD 921600
#endif
int uart_set_baudrate(int port,unsigned rate){assert(port==0&&rate==CONFIG_OSG_SERIAL_BAUD);return 0;}
static bool installed;
static int install_error, line_error, installs, deletes, uses;
static int task_error, event_count, wake_count, task_deleted;
static void (*worker)(void *);
static int queue_token, owner_token;
TaskHandle_t xTaskGetCurrentTaskHandle(void){return &owner_token;}
int xTaskCreate(void (*run)(void *),const char *name,unsigned stack,void *owner,unsigned priority,void *task){
    assert(name&&stack>=2048&&owner==&owner_token&&priority==5&&!task);
    if(task_error)return 0;
    worker=run;return pdPASS;
}
int xQueueReceive(QueueHandle_t queue,void *event,unsigned delay){
    assert(queue==&queue_token&&event&&delay==portMAX_DELAY);
    if(!event_count)return 0;
    event_count--;return pdTRUE;
}
void xTaskNotifyGive(TaskHandle_t owner){assert(owner==&owner_token);wake_count++;}
void vTaskDelete(TaskHandle_t owner){assert(!owner);task_deleted++;}
bool uart_is_driver_installed(int port){assert(port==0);return installed;}
int uart_driver_install(int port,int rx,int tx,int queue,QueueHandle_t *events,int flags){
    assert(port==0&&rx==9216&&!tx&&queue==16&&events&&!flags);
    *events=&queue_token;installs++;return install_error;
}
int uart_driver_delete(int port){assert(port==0);deletes++;return 0;}
int uart_vfs_dev_port_set_rx_line_endings(int port,int mode){assert(port==0&&mode==0);return line_error;}
void uart_vfs_dev_use_driver(int port){assert(port==0);uses++;}
int main(void){
#if CONFIG_ESP_CONSOLE_UART
    assert(osg_serial_start()&&uses==1&&installs==1&&!deletes);
    installed=true;assert(!osg_serial_start()&&installs==1);installed=false;
    install_error=-1;assert(!osg_serial_start()&&uses==1&&installs==2&&!deletes);install_error=0;
    line_error=-1;assert(!osg_serial_start()&&uses==1&&deletes==1);line_error=0;
    assert(osg_serial_start()&&uses==2&&installs==4);
    task_error=1;assert(!osg_serial_start()&&uses==2&&deletes==2);
    assert(osg_serial_pending()&&!osg_serial_pending());
    event_count=2;worker(&owner_token);
    assert(wake_count==1&&task_deleted==1);
    assert(osg_serial_pending()&&!osg_serial_pending());
#else
    assert(osg_serial_start()&&!installs&&!uses&&!deletes);
    assert(osg_serial_pending()&&osg_serial_pending());
#endif
    puts("Serial RX capacity, startup failures and non-UART console checks passed");
}
