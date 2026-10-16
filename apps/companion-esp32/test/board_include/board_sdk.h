#pragma once
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
typedef int esp_err_t;
#define ESP_OK 0
#define CONFIG_IDF_TARGET_ESP32S3 1
#define CONFIG_SPIRAM 1
#define CONFIG_SPIRAM_MODE_OCT 1
#define CONFIG_SPIRAM_XIP_FROM_PSRAM 1
#define GPIO_MODE_OUTPUT 1
#define GPIO_MODE_INPUT 2
#define GPIO_PULLUP_DISABLE 0
#define GPIO_PULLDOWN_DISABLE 0
#define GPIO_INTR_DISABLE 0
#define I2C_CLK_SRC_DEFAULT 0
#define I2C_ADDR_BIT_LEN_7 0
#define LCD_CLK_SRC_DEFAULT 0
static inline void mock_log(const char *tag,const char *format,...){(void)tag;(void)format;}
#define ESP_LOGE(...) mock_log(__VA_ARGS__)
#define ESP_LOGI(...) mock_log(__VA_ARGS__)
#define pdFALSE 0
#define pdTRUE 1
#define pdMS_TO_TICKS(ms) (ms)
typedef int BaseType_t;
typedef void *SemaphoreHandle_t;
typedef void *i2c_master_bus_handle_t;
typedef void *i2c_master_dev_handle_t;
typedef void *esp_lcd_panel_handle_t;
typedef struct {int unused;} esp_lcd_rgb_panel_event_data_t;
typedef struct {uint64_t pin_bit_mask; int mode,pull_up_en,pull_down_en,intr_type;} gpio_config_t;
typedef struct {int i2c_port,sda_io_num,scl_io_num,clk_source,glitch_ignore_cnt; struct {bool enable_internal_pullup;} flags;} i2c_master_bus_config_t;
typedef struct {int dev_addr_length;unsigned device_address,scl_speed_hz;} i2c_device_config_t;
typedef struct {
 int clk_src; struct {unsigned pclk_hz,h_res,v_res,hsync_pulse_width,hsync_back_porch,hsync_front_porch,vsync_pulse_width,vsync_back_porch,vsync_front_porch; struct {bool pclk_active_neg;} flags;} timings;
 unsigned data_width,bits_per_pixel,num_fbs,bounce_buffer_size_px,dma_burst_size;
 int hsync_gpio_num,vsync_gpio_num,de_gpio_num,pclk_gpio_num,disp_gpio_num,data_gpio_nums[16];
 struct {bool fb_in_psram;} flags;
} esp_lcd_rgb_panel_config_t;
typedef bool (*vsync_callback)(esp_lcd_panel_handle_t,const esp_lcd_rgb_panel_event_data_t *,void *);
typedef struct {vsync_callback on_vsync;} esp_lcd_rgb_panel_event_callbacks_t;
size_t esp_psram_get_size(void);
int64_t esp_timer_get_time(void);
void vTaskDelay(unsigned);
SemaphoreHandle_t xSemaphoreCreateCounting(unsigned,unsigned);
int xSemaphoreTake(SemaphoreHandle_t,unsigned);
int xSemaphoreGiveFromISR(SemaphoreHandle_t,BaseType_t *);
void vSemaphoreDelete(SemaphoreHandle_t);
esp_err_t gpio_config(const gpio_config_t *);
esp_err_t gpio_set_level(int,int);
esp_err_t gpio_set_direction(int,int);
esp_err_t gpio_reset_pin(int);
esp_err_t i2c_new_master_bus(const i2c_master_bus_config_t *,i2c_master_bus_handle_t *);
esp_err_t i2c_del_master_bus(i2c_master_bus_handle_t);
esp_err_t i2c_master_bus_add_device(i2c_master_bus_handle_t,const i2c_device_config_t *,i2c_master_dev_handle_t *);
esp_err_t i2c_master_bus_rm_device(i2c_master_dev_handle_t);
esp_err_t i2c_master_transmit(i2c_master_dev_handle_t,const void *,size_t,int);
esp_err_t i2c_master_transmit_receive(i2c_master_dev_handle_t,const void *,size_t,void *,size_t,int);
esp_err_t esp_lcd_new_rgb_panel(const esp_lcd_rgb_panel_config_t *,esp_lcd_panel_handle_t *);
esp_err_t esp_lcd_rgb_panel_get_frame_buffer(esp_lcd_panel_handle_t,unsigned,void **,...);
esp_err_t esp_lcd_rgb_panel_register_event_callbacks(esp_lcd_panel_handle_t,const esp_lcd_rgb_panel_event_callbacks_t *,void *);
esp_err_t esp_lcd_panel_reset(esp_lcd_panel_handle_t);
esp_err_t esp_lcd_panel_init(esp_lcd_panel_handle_t);
esp_err_t esp_lcd_panel_del(esp_lcd_panel_handle_t);
esp_err_t esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t,int,int,int,int,const void *);
