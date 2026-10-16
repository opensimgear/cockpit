#pragma once
#define ESP_LINE_ENDINGS_LF 0
int uart_vfs_dev_port_set_rx_line_endings(int,int);
void uart_vfs_dev_use_driver(int);
