#pragma once
typedef void *QueueHandle_t;
int xQueueReceive(QueueHandle_t, void *, unsigned);
