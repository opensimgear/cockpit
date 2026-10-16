#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define CONFIG_IDF_TARGET_ESP32 1
#define CONFIG_OSG_SUNTON_X_MIN 200
#define CONFIG_OSG_SUNTON_X_MAX 3900
#define CONFIG_OSG_SUNTON_Y_MIN 200
#define CONFIG_OSG_SUNTON_Y_MAX 3900
#define ESP_OK 0
#define GPIO_MODE_OUTPUT 1
#define GPIO_MODE_INPUT 2
#define SPI2_HOST 2
#define SPI_DMA_DISABLED 0
#define pdTRUE 1
#define pdMS_TO_TICKS(x) (x)
#define ESP_LOGI(...) ((void)0)
typedef int esp_err_t;
typedef void *SemaphoreHandle_t;
typedef void *spi_device_handle_t;
typedef struct {uint64_t pin_bit_mask;int mode;} gpio_config_t;
typedef struct {int mosi_io_num,miso_io_num,sclk_io_num,quadwp_io_num,quadhd_io_num,max_transfer_sz;} spi_bus_config_t;
typedef struct {int clock_speed_hz,mode,spics_io_num,queue_size;} spi_device_interface_config_t;
typedef struct {size_t length;const void *tx_buffer;void *rx_buffer;} spi_transaction_t;
int gpio_config(const gpio_config_t *);
int gpio_set_level(int,int);
int gpio_get_level(int);
SemaphoreHandle_t xSemaphoreCreateMutex(void);
int xSemaphoreTake(SemaphoreHandle_t,unsigned);
int xSemaphoreGive(SemaphoreHandle_t);
void vTaskDelay(unsigned);
int spi_bus_initialize(int,const spi_bus_config_t *,int);
int spi_bus_add_device(int,const spi_device_interface_config_t *,spi_device_handle_t *);
int spi_device_polling_transmit(spi_device_handle_t,spi_transaction_t *);
