#pragma once
#include <stdbool.h>
#include <stddef.h>
#include "freertos/queue.h"
#define ESP_OK 0
typedef enum {UART_DATA,UART_BREAK,UART_BUFFER_FULL,UART_FIFO_OVF,UART_FRAME_ERR,UART_PARITY_ERR,UART_EVENT_MAX} uart_event_type_t;
typedef struct {uart_event_type_t type;size_t size;bool timeout_flag;} uart_event_t;
bool uart_is_driver_installed(int);
int uart_driver_install(int,int,int,int,QueueHandle_t *,int);
int uart_driver_delete(int);
int uart_set_baudrate(int,unsigned);
int uart_set_rx_full_threshold(int,int);
