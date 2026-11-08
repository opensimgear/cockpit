#ifndef OSG_BOARD_H
#define OSG_BOARD_H
#include "raster.h"
typedef struct {
    uint16_t width, height;
    void *context;
    osg_write_pixels write;
    /* Poll calibrated, rotated screen coordinates. Return false on touch I/O
     * failure/cancellation; caller disarms. NULL means display-only. */
    bool (*touch)(void *, bool *down, int *x, int *y);
    /* Optional bounded presentation fence after all writes, outside the state mutex. */
    bool (*present)(void *);
    /* Optional touch interrupt input. Zero leaves the existing timed reader. */
    int touch_irq_gpio;
    /* Active-low PENIRQ is required and stays low for an unqualified contact. */
    bool touch_penirq;
} osg_board;
bool osg_board_init(osg_board *);
#endif
