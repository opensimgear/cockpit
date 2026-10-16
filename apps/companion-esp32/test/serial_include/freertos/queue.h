#pragma once
#include <stddef.h>
typedef void *QueueHandle_t;
#define pdTRUE 1
int xQueueReceive(QueueHandle_t,void *,unsigned);
unsigned uxQueueSpacesAvailable(QueueHandle_t);
