#pragma once
typedef void *SemaphoreHandle_t;
int xSemaphoreTake(SemaphoreHandle_t,unsigned);
int xSemaphoreGive(SemaphoreHandle_t);
