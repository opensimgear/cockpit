#pragma once
#include <stdint.h>
#define portMAX_DELAY 0xffffffffu
#define pdPASS 1
#define pdMS_TO_TICKS(n) (n)
void vTaskDelay(unsigned);
int xTaskCreate(void (*)(void *),const char *,unsigned,void *,unsigned,void *);
