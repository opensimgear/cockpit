#pragma once
#define ESP_OK 0
#define ESP_ERR_NVS_NOT_FOUND 1
typedef int esp_err_t;
esp_err_t nvs_flash_init(void);
