#include "serial.h"
#include "settings.h"
#include "sdkconfig.h"
#include <stddef.h>
#include <string.h>
#if CONFIG_ESP_CONSOLE_UART
#include "driver/uart.h"
#include "driver/uart_vfs.h"
#include "freertos/queue.h"
static QueueHandle_t events;
#define EVENT_CAPACITY 16u
#ifdef ESP_PLATFORM
/* Includes two heap-block overhead allowances; measured task stacks preexist. */
_Static_assert(sizeof(osg_serial_health)+sizeof(events)+4+EVENT_CAPACITY*sizeof(uart_event_t)+sizeof(StaticQueue_t)+64<=OSG_SERIAL_HEALTH_RESERVE_BYTES,"Serial health exceeds512B reserve");
#endif
#endif
#ifndef CONFIG_OSG_SERIAL_BAUD
#define CONFIG_OSG_SERIAL_BAUD 921600
#endif
static osg_serial_health health;
static void increment(uint32_t *value){if(*value<UINT32_MAX)(*value)++;}
void osg_serial_health_failure(osg_serial_failure failure){
    uint32_t *value=NULL;
    switch(failure){
    case OSG_SERIAL_TIMEOUT:value=&health.request_timeouts;break;
    case OSG_SERIAL_JSON:value=&health.invalid_json;break;
    case OSG_SERIAL_IDENTITY:value=&health.reply_mismatch;break;
    case OSG_SERIAL_DEFERRED:value=&health.deferred_full;break;
    case OSG_SERIAL_REPLY:value=&health.invalid_reply;break;
    case OSG_SERIAL_WRITE:value=&health.write_failures;break;
    case OSG_SERIAL_FRAME:value=&health.frame_overflows;break;
    default:break;
    }
    if(value)increment(value);
}
void osg_serial_health_poll(void){
#if CONFIG_ESP_CONSOLE_UART
    if(!events)return;
    if(!uxQueueSpacesAvailable(events))increment(&health.queue_full_observations);
    for(unsigned i=0;i<EVENT_CAPACITY;i++){
        uart_event_t event;if(xQueueReceive(events,&event,0)!=pdTRUE)break;
        switch(event.type){
        case UART_FIFO_OVF:increment(&health.fifo_overflows);break;
        case UART_BUFFER_FULL:increment(&health.buffer_full);break;
        case UART_FRAME_ERR:increment(&health.frame_errors);break;
        case UART_PARITY_ERR:increment(&health.parity_errors);break;
        case UART_BREAK:increment(&health.breaks);break;
        default:break;
        }
    }
#endif
}
void osg_serial_health_snapshot(osg_serial_health *out){if(out)*out=health;}
bool osg_serial_start(void) {
#if CONFIG_ESP_CONSOLE_UART
    /* Keep a full bounded record in the ring. At921600 baud a120-byte FIFO
     * trigger leaves only8 bytes (~87us);64 leaves ~694us before overflow.
     * The small event queue reports errors without an extra task or ISR hook. */
    const int port=CONFIG_ESP_CONSOLE_UART_NUM;
    if(uart_is_driver_installed(port))return false;
    events=NULL;
    if(uart_driver_install(port,OSG_PROVISION_BYTES+1024,0,EVENT_CAPACITY,&events,0)!=ESP_OK){events=NULL;return false;}
    if(uart_vfs_dev_port_set_rx_line_endings(port,ESP_LINE_ENDINGS_LF)!=0||
       uart_set_baudrate(port,CONFIG_OSG_SERIAL_BAUD)!=ESP_OK||uart_set_rx_full_threshold(port,64)!=ESP_OK){
        (void)uart_driver_delete(port);events=NULL;return false;
    }
    uart_vfs_dev_use_driver(port);
#endif
    memset(&health,0,sizeof(health));return true;
}
