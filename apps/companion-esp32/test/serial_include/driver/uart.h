#pragma once
#include <stdbool.h>
#define ESP_OK 0
bool uart_is_driver_installed(int);
int uart_driver_install(int,int,int,int,void *,int);
int uart_driver_delete(int);

int uart_set_baudrate(int,unsigned);
