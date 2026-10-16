#ifndef OSG_RASTER_H
#define OSG_RASTER_H
#include "companion.h"
/* CPU-order RGB565. The board must copy/consume pixels before returning, including
 * waiting for queued DMA completion. Converts to RGB888/monochrome in its adapter. */
typedef bool (*osg_write_pixels)(void *, int x, int y, int width, int height, const uint16_t *pixels);
typedef struct { void *context; osg_write_pixels write; bool failed; uint16_t scratch[256]; bool composing; int tile_x,tile_y,tile_width,tile_height; uint16_t tile[512]; } osg_raster;
osg_display osg_raster_display(osg_raster *, uint16_t width, uint16_t height);
/* Buffered 64x8 tiles, synchronous board writes; failure latches failed.
 * After recovery the caller must retry a full frame before admitting input. */
void osg_raster_render(osg_raster *, osg_runtime *, osg_damage, uint64_t now_ms);
#endif
