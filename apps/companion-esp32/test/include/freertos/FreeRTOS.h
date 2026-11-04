#pragma once
#include <stdint.h>
#define portMAX_DELAY 0xffffffffu
#define pdPASS 1
#define pdMS_TO_TICKS(n) (n)
#define pdTRUE 1
typedef void *TaskHandle_t;
void vTaskDelay(unsigned);
void vTaskDelete(TaskHandle_t);
TaskHandle_t xTaskGetCurrentTaskHandle(void);
void xTaskNotifyGive(TaskHandle_t);
unsigned ulTaskNotifyTake(int,unsigned);
int xTaskCreate(void (*)(void *),const char *,unsigned,void *,unsigned,void *);
