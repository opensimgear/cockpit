/* Waveshare ESP32-S3-Touch-LCD-7 V1.2; hardware sources in README.md.
 * Original adapter, using only ESP-IDF APIs (no LVGL/managed components). */
#include "board.h"
#include "driver/gpio.h"
#include "driver/i2c_master.h"
#include "esp_lcd_panel_rgb.h"
#include "esp_lcd_panel_ops.h"
#include "esp_log.h"
#include "esp_psram.h"
#include "esp_timer.h"
#include "freertos/FreeRTOS.h"
#include "freertos/semphr.h"
#include "freertos/task.h"
#include "sdkconfig.h"
#include <string.h>
#if !CONFIG_IDF_TARGET_ESP32S3 || !CONFIG_SPIRAM || !CONFIG_SPIRAM_MODE_OCT || !CONFIG_SPIRAM_XIP_FROM_PSRAM
#error "Waveshare LCD7 requires ESP32-S3 with octal PSRAM and PSRAM XIP; use its sdkconfig.defaults"
#endif
#define WIDTH 800
#define HEIGHT 480
#define FRAME_BYTES ((size_t)WIDTH * HEIGHT * sizeof(uint16_t))
#define I2C_TIMEOUT_MS 30
#define TOUCH_HOLD_MS 200
/* CH422G IO1 touch reset, IO2 backlight, IO3 LCD reset, IO4 SD CS,
 * IO5 USB select. Keep SD deselected and USB select high throughout. */
#define OUTPUTS_RESET 0x30
#define OUTPUTS_LCD_READY 0x38
#define OUTPUTS_TOUCH_READY 0x3a
#define OUTPUTS_LIGHT_ON 0x3e
typedef struct {
    i2c_master_bus_handle_t bus;
    i2c_master_dev_handle_t config, outputs, touch;
    esp_lcd_panel_handle_t panel;
    SemaphoreHandle_t vsync;
    uint16_t *fb[2];
    unsigned back;
    bool initialized, display_failed, touch_known, down;
    uint8_t contact;
    int x, y;
    uint64_t report_ms;
} lcd7;
static lcd7 hardware;

static bool expander(i2c_master_dev_handle_t device, uint8_t value) {
    return i2c_master_transmit(device, &value, 1, I2C_TIMEOUT_MS) == ESP_OK;
}
static bool read_touch(lcd7 *b, uint16_t reg, void *data, size_t bytes) {
    uint8_t address[] = {(uint8_t)(reg >> 8), (uint8_t)reg};
    return i2c_master_transmit_receive(b->touch, address, sizeof(address), data, bytes, I2C_TIMEOUT_MS) == ESP_OK;
}
static bool clear_touch(lcd7 *b) {
    const uint8_t clear[] = {0x81, 0x4e, 0};
    return i2c_master_transmit(b->touch, clear, sizeof(clear), I2C_TIMEOUT_MS) == ESP_OK;
}
static bool touch_failure(lcd7 *b) { b->touch_known = false; b->down = false; return false; }
static bool poll_touch(void *context, bool *down, int *x, int *y) {
    lcd7 *b = context;
    *down = false; *x = *y = 0;
    if (!b->initialized) return false;
    uint8_t status;
    if (!read_touch(b, 0x814e, &status, 1)) return touch_failure(b);
    const uint64_t now = (uint64_t)(esp_timer_get_time() / 1000);
    if (status & 0x80) {
        uint8_t point[8] = {0};
        const unsigned count = status & 15;
        bool valid = !(status & 0x70) && count <= 1;
        if (valid && count) valid = read_touch(b, 0x814f, point, sizeof(point));
        /* Consume ready reports even on unsupported multitouch. Never choose
         * an arbitrary finger or fabricate a release after a failed read. */
        if (!clear_touch(b) || !valid) return touch_failure(b);
        if (count) {
            const int px = point[1] | point[2] << 8, py = point[3] | point[4] << 8;
            if (px >= WIDTH || py >= HEIGHT || (b->touch_known && b->down && b->contact != point[0]))
                return touch_failure(b);
            b->contact = point[0]; b->x = px; b->y = py;
        }
        b->down = count != 0; b->touch_known = true; b->report_ms = now;
    }
    /* No-ready means no new report, not "released". A cached held point can
     * survive short gaps, but successful I2C reads cannot extend it forever. */
    if (!b->touch_known || (b->down && (now < b->report_ms || now - b->report_ms >= TOUCH_HOLD_MS)))
        return touch_failure(b);
    *down = b->down; *x = b->x; *y = b->y; return true;
}
static bool write_pixels(void *context, int x, int y, int w, int h, const uint16_t *pixels) {
    lcd7 *b = context;
    if (!b->initialized || b->display_failed || !pixels || x < 0 || y < 0 ||
        w <= 0 || h <= 0 || x > WIDTH - w || y > HEIGHT - h) return false;
    for (int row = 0; row < h; row++)
        memcpy(b->fb[b->back] + (size_t)(y + row) * WIDTH + x, pixels + (size_t)row * w, (size_t)w * sizeof(*pixels));
    return true; /* Caller scratch is fully copied before return. */
}
static bool on_vsync(esp_lcd_panel_handle_t panel, const esp_lcd_rgb_panel_event_data_t *event, void *context) {
    (void)panel; (void)event;
    lcd7 *b = context; BaseType_t wake = pdFALSE;
    (void)xSemaphoreGiveFromISR(b->vsync, &wake); return wake == pdTRUE;
}
static bool present(void *context) {
    lcd7 *b = context;
    if (!b->initialized || b->display_failed) return false;
    /* Two IDF-owned PSRAM frames. The bounce driver switches its source at a
     * frame boundary. Wait three *new* VSYNCs before reusing the old frame:
     * this includes a boundary racing submission plus a full new scanout.
     * Timeout latches failure; never recycle a potentially scanned buffer. */
    for (unsigned i = 0; i < 3; i++) (void)xSemaphoreTake(b->vsync, 0);
    if (esp_lcd_panel_draw_bitmap(b->panel, 0, 0, WIDTH, HEIGHT, b->fb[b->back]) != ESP_OK)
        goto failed;
    for (unsigned i = 0; i < 3; i++)
        if (xSemaphoreTake(b->vsync, pdMS_TO_TICKS(80)) != pdTRUE) goto failed;
    /* Partial redraws leave most pixels untouched. The recycled frame must
     * contain the newly presented image before the next write, otherwise
     * alternating disjoint updates restore stale pixels every other frame.
     * The fence above must complete before writing the old scanout frame. */
    const unsigned recycled = 1 - b->back;
    memcpy(b->fb[recycled], b->fb[b->back], FRAME_BYTES);
    b->back = recycled; return true;
failed:
    b->display_failed = true;
    ESP_LOGE("lcd7", "Frame presentation failed; reboot required");
    return false;
}
static void cleanup(lcd7 *b) {
    if (b->outputs) (void)expander(b->outputs, OUTPUTS_TOUCH_READY); /* backlight off */
    if (b->panel) (void)esp_lcd_panel_del(b->panel); /* Stop ISR before deleting semaphore. */
    if (b->vsync) vSemaphoreDelete(b->vsync);
    if (b->touch) (void)i2c_master_bus_rm_device(b->touch);
    if (b->outputs) (void)i2c_master_bus_rm_device(b->outputs);
    if (b->config) (void)i2c_master_bus_rm_device(b->config);
    if (b->bus) (void)i2c_del_master_bus(b->bus);
    (void)gpio_reset_pin(4);
    memset(b, 0, sizeof(*b));
}
static bool add_device(lcd7 *b, unsigned address, i2c_master_dev_handle_t *out) {
    const i2c_device_config_t config = {
        .dev_addr_length = I2C_ADDR_BIT_LEN_7, .device_address = address, .scl_speed_hz = 400000
    };
    return i2c_master_bus_add_device(b->bus, &config, out) == ESP_OK;
}
bool osg_board_init(osg_board *out) {
    lcd7 *b = &hardware;
    *out = (osg_board){0};
    if (b->initialized) return false;
    if (esp_psram_get_size() < 8 * 1024 * 1024) {
        ESP_LOGE("lcd7", "Expected N8R8 board with 8 MiB PSRAM"); return false;
    }
    const char *stage = "I2C/CH422G setup";
    const i2c_master_bus_config_t bus = {
        .i2c_port = 0, .sda_io_num = 8, .scl_io_num = 9,
        .clk_source = I2C_CLK_SRC_DEFAULT, .glitch_ignore_cnt = 7,
        .flags.enable_internal_pullup = true
    };
    if (i2c_new_master_bus(&bus, &b->bus) != ESP_OK ||
        !add_device(b, 0x24, &b->config) || !add_device(b, 0x38, &b->outputs) ||
        !add_device(b, 0x5d, &b->touch) || !expander(b->config, 1)) goto failed;
    stage = "reset/address selection";
    const gpio_config_t irq = {.pin_bit_mask = 1ULL << 4, .mode = GPIO_MODE_OUTPUT,
        .pull_up_en = GPIO_PULLUP_DISABLE, .pull_down_en = GPIO_PULLDOWN_DISABLE, .intr_type = GPIO_INTR_DISABLE};
    if (gpio_config(&irq) != ESP_OK || gpio_set_level(4, 0) != ESP_OK ||
        !expander(b->outputs, OUTPUTS_RESET)) goto failed;
    vTaskDelay(pdMS_TO_TICKS(20));
    if (!expander(b->outputs, OUTPUTS_LCD_READY)) goto failed;
    vTaskDelay(pdMS_TO_TICKS(100));
    if (!expander(b->outputs, OUTPUTS_TOUCH_READY)) goto failed;
    vTaskDelay(pdMS_TO_TICKS(50));
    if (gpio_set_direction(4, GPIO_MODE_INPUT) != ESP_OK) goto failed; /* GT911 INT, not a driven output. */
    vTaskDelay(pdMS_TO_TICKS(150));
    stage = "GT911 identity/800x480 geometry";
    uint8_t product[4], geometry[4];
    if (!read_touch(b, 0x8140, product, sizeof(product)) || memcmp(product, "911", 3) ||
        !read_touch(b, 0x8048, geometry, sizeof(geometry)) ||
        (geometry[0] | geometry[1] << 8) != WIDTH || (geometry[2] | geometry[3] << 8) != HEIGHT) goto failed;
    stage = "VSYNC semaphore";
    b->vsync = xSemaphoreCreateCounting(3, 0);
    if (!b->vsync) goto failed;
    const esp_lcd_rgb_panel_config_t rgb = {
        .clk_src = LCD_CLK_SRC_DEFAULT,
        .timings = {.pclk_hz = 16000000, .h_res = WIDTH, .v_res = HEIGHT,
            .hsync_pulse_width = 4, .hsync_back_porch = 8, .hsync_front_porch = 8,
            .vsync_pulse_width = 4, .vsync_back_porch = 8, .vsync_front_porch = 8,
            .flags.pclk_active_neg = true},
        .data_width = 16, .bits_per_pixel = 16, .num_fbs = 2,
        .bounce_buffer_size_px = WIDTH * 10, .dma_burst_size = 64,
        .hsync_gpio_num = 46, .vsync_gpio_num = 3, .de_gpio_num = 5, .pclk_gpio_num = 7, .disp_gpio_num = -1,
        .data_gpio_nums = {14,38,18,17,10,39,0,45,48,47,21,1,2,42,41,40},
        .flags.fb_in_psram = true
    };
    const esp_lcd_rgb_panel_event_callbacks_t callbacks = {.on_vsync = on_vsync};
    stage = "RGB frame allocation/callbacks";
    if (esp_lcd_new_rgb_panel(&rgb, &b->panel) != ESP_OK ||
        esp_lcd_rgb_panel_get_frame_buffer(b->panel, 2, (void **)&b->fb[0], (void **)&b->fb[1]) != ESP_OK ||
        esp_lcd_rgb_panel_register_event_callbacks(b->panel, &callbacks, b) != ESP_OK) goto failed;
    memset(b->fb[0], 0, FRAME_BYTES); memset(b->fb[1], 0, FRAME_BYTES); b->back = 1;
    stage = "RGB startup/backlight";
    if (esp_lcd_panel_reset(b->panel) != ESP_OK || esp_lcd_panel_init(b->panel) != ESP_OK ||
        !expander(b->outputs, OUTPUTS_LIGHT_ON)) goto failed;
    b->initialized = true;
    *out = (osg_board){.width = WIDTH, .height = HEIGHT, .context = b,
        .write = write_pixels, .touch = poll_touch, .present = present};
    ESP_LOGI("lcd7", "Waveshare ESP32-S3-Touch-LCD-7 V1.2: RGB565 800x480, GT911, two PSRAM frames");
    return true;
failed:
    ESP_LOGE("lcd7", "Board initialization failed at %s; display/touch unavailable", stage);
    cleanup(b); return false;
}
