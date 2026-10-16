#ifndef OSG_RASTER_H
#define OSG_RASTER_H
#include "companion.h"
/* CPU-order RGB565. The board must copy/consume pixels before returning, including
 * waiting for queued DMA completion. Converts to RGB888/monochrome in its adapter. */
typedef bool (*osg_write_pixels)(void *, int x, int y, int width, int height, const uint16_t *pixels);
typedef struct { void *context; osg_write_pixels write; bool failed; uint16_t scratch[256]; } osg_raster;
osg_display osg_raster_display(osg_raster *, uint16_t width, uint16_t height);
#endif
