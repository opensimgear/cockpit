#include "serial.h"
#include "settings.h"
#include "sdkconfig.h"
#if CONFIG_ESP_CONSOLE_UART
#include "driver/uart.h"
#include "driver/uart_vfs.h"
#endif
#ifndef CONFIG_OSG_SERIAL_BAUD
#define CONFIG_OSG_SERIAL_BAUD 921600
#endif
bool osg_serial_start(void) {
#if CONFIG_ESP_CONSOLE_UART
    /* The network task can block on TLS/ACKs. Keep a complete bounded serial
     * record in an interrupt-driven ring instead of the small hardware FIFO. */
    const int port = CONFIG_ESP_CONSOLE_UART_NUM;
    if (uart_is_driver_installed(port)) return false;
    if (uart_driver_install(port, OSG_PROVISION_BYTES + 1024, 0, 0, NULL, 0) != ESP_OK) return false;
    if (uart_vfs_dev_port_set_rx_line_endings(port, ESP_LINE_ENDINGS_LF) != 0) {
        (void)uart_driver_delete(port); return false;
    }
    if (uart_set_baudrate(port, CONFIG_OSG_SERIAL_BAUD) != ESP_OK) {(void)uart_driver_delete(port); return false;}
    uart_vfs_dev_use_driver(port);
#endif
    return true;
}
