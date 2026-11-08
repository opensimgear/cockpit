#include "serial.h"
#include "settings.h"
#include "sdkconfig.h"
#if !CONFIG_ESP_CONSOLE_UART
#error "ESP32 companion firmware requires a UART console with receive notifications"
#endif
#include "driver/uart.h"
#include "driver/uart_vfs.h"
#include "freertos/queue.h"
#include "freertos/task.h"
#include <stdatomic.h>
static QueueHandle_t receive_events;
static atomic_uint receive_pending = 1;

static void serial_events(void *owner) {
    uart_event_t event;
    while (xQueueReceive(receive_events, &event, portMAX_DELAY) == pdTRUE) {
        // Also wake on error events. If the bounded event queue fills, a
        // surviving event still prompts the sole reader to drain the ring.
        if (atomic_exchange(&receive_pending, 1) == 0) xTaskNotifyGive(owner);
    }
    vTaskDelete(NULL);
}
#ifndef CONFIG_OSG_SERIAL_BAUD
#define CONFIG_OSG_SERIAL_BAUD 921600
#endif
bool osg_serial_start(void) {
    /* The network task can block on TLS/ACKs. Keep a complete bounded serial
     * record in an interrupt-driven ring instead of the small hardware FIFO. */
    const int port = CONFIG_ESP_CONSOLE_UART_NUM;
    if (uart_is_driver_installed(port)) return false;
    if (uart_driver_install(port, OSG_PROVISION_BYTES + 1024, 0, 16, &receive_events, 0) != ESP_OK) return false;
    if (uart_vfs_dev_port_set_rx_line_endings(port, ESP_LINE_ENDINGS_LF) != 0) {
        (void)uart_driver_delete(port); return false;
    }
    if (uart_set_baudrate(port, CONFIG_OSG_SERIAL_BAUD) != ESP_OK) {(void)uart_driver_delete(port); return false;}
    if (xTaskCreate(serial_events, "companion-uart", 2048, xTaskGetCurrentTaskHandle(), 5, NULL) != pdPASS) {
        (void)uart_driver_delete(port);
        return false;
    }
    uart_vfs_dev_use_driver(port);
    return true;
}
bool osg_serial_pending(void) {
    return atomic_exchange(&receive_pending, 0) != 0;
}
