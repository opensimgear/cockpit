#include "esp_lcd_bridge.h"
bool osg_lcd_write(void *context, int x, int y, int w, int h, const uint16_t *pixels) {
    osg_lcd_bridge *b = context;
    if (!b || !b->panel || !b->wait_complete || !pixels || w <= 0 || h <= 0) return false;
    if (esp_lcd_panel_draw_bitmap(b->panel, x, y, x + w, y + h, pixels) != ESP_OK) return false;
    return b->wait_complete(b->context);
}
