#include "board.h"
#include "sdkconfig.h"
#include "esp_log.h"
static bool discard(void *ctx, int x, int y, int w, int h, const uint16_t *pixels) {
    (void)ctx; (void)x; (void)y; (void)w; (void)h; (void)pixels;
    return true;
}
bool osg_board_init(osg_board *out) {
    *out = (osg_board){.width = CONFIG_OSG_SCREEN_WIDTH, .height = CONFIG_OSG_SCREEN_HEIGHT, .write = discard};
    ESP_LOGW("companion", "HEADLESS build: no physical display driver, pins or touch configured");
    return true;
}
