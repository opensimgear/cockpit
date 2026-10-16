#ifndef TEST_ESP_LCD_OPS_H
#define TEST_ESP_LCD_OPS_H
#include <stdint.h>
typedef void *esp_lcd_panel_handle_t;
#define ESP_OK 0
int esp_lcd_panel_draw_bitmap(esp_lcd_panel_handle_t, int, int, int, int, const void *);
#endif
