#pragma once
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
SemaphoreHandle_t xSemaphoreCreateMutex(void);
unsigned uxTaskGetStackHighWaterMark(void *);
