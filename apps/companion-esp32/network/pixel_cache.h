#ifndef OSG_PIXEL_CACHE_H
#define OSG_PIXEL_CACHE_H
#include <stdbool.h>
#include <stddef.h>

/* Owners are the immutable active/candidate panel images. Release an owner
 * only after the renderer has stopped reading that panel. */
bool osg_pixels_panel_begin(unsigned owner);
void osg_pixels_panel_release(unsigned owner);
bool osg_pixels_contains(const char *content_hash, unsigned width, unsigned height, size_t bytes);
bool osg_pixels_acquire(unsigned owner, const char *content_hash,
                        unsigned width, unsigned height, size_t bytes,
                        unsigned *asset, bool *cached);
bool osg_pixels_write(unsigned asset, size_t offset, const void *, size_t count);
const unsigned char *osg_pixels_finish(unsigned asset);
/* Stable machine-readable reason for the last failed operation. */
const char *osg_pixels_error(void);
#endif
