#ifndef OSG_ESP_LCD_BRIDGE_H
#define OSG_ESP_LCD_BRIDGE_H
#include "board.h"
#include "esp_lcd_panel_ops.h"
/* wait_complete must wait for on_color_trans_done (SPI/I80) or the appropriate
 * RGB flush fence. Board chooses CPU-order RGB565/endian conversion at setup.
 * The bridge never retains the caller's scratch buffer after returning. */
typedef struct {
    esp_lcd_panel_handle_t panel;
    void *context;
    bool (*wait_complete)(void *);
} osg_lcd_bridge;
bool osg_lcd_write(void *, int, int, int, int, const uint16_t *);
#endif
