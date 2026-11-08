#ifndef OSG_BOARD_H
#define OSG_BOARD_H
#include "raster.h"
enum { OSG_TOUCH_RECHECK_MS = 20, OSG_TOUCH_HEALTH_MS = 200 };
typedef struct {
    uint16_t width, height;
    void *context;
    osg_write_pixels write;
    /* Poll calibrated, rotated screen coordinates. Return false on touch I/O
     * failure/cancellation; caller disarms. NULL means display-only. */
    bool (*touch)(void *, bool *down, int *x, int *y);
    /* Optional bounded presentation fence after all writes, outside the state mutex. */
    bool (*present)(void *);
    /* Touch-capable boards supply an interrupt pin and the next read deadline. */
    int touch_irq_gpio;
    unsigned (*touch_wait_ms)(void *, bool valid, bool down, bool notified);
} osg_board;
bool osg_board_init(osg_board *);
#endif
