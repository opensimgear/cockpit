#pragma once
#include <stdbool.h>
#include "freertos/queue.h"
#define ESP_OK 0
typedef struct { int type; } uart_event_t;
bool uart_is_driver_installed(int);
int uart_driver_install(int,int,int,int,QueueHandle_t *,int);
int uart_driver_delete(int);

int uart_set_baudrate(int,unsigned);
