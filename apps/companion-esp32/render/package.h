#ifndef OSG_RENDER_PACKAGE_H
#define OSG_RENDER_PACKAGE_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define OSG_RENDER_OBJECT_HEADER_BYTES 32u
#define OSG_RENDER_DIRECTORY_RECORD_BYTES 16u
#define OSG_RENDER_MAX_SECTIONS 16u

typedef enum {
    OSG_RENDER_SEMANTICS = 0,
    OSG_RENDER_SCENE,
    OSG_RENDER_IMAGES,
    OSG_RENDER_FONTS,
    OSG_RENDER_OBJECT_KINDS
} osg_render_object_kind;

typedef enum {
    OSG_RENDER_PACKAGE_OK = 0,
    OSG_RENDER_PACKAGE_ARGUMENT,
    OSG_RENDER_PACKAGE_READ,
    OSG_RENDER_PACKAGE_HEADER,
    OSG_RENDER_PACKAGE_SIZE,
    OSG_RENDER_PACKAGE_DIRECTORY,
    OSG_RENDER_PACKAGE_SECTION,
    OSG_RENDER_PACKAGE_OVERLAP,
    OSG_RENDER_PACKAGE_BUDGET
} osg_render_package_status;

/* Copy exactly bytes into destination before returning. The caller guarantees
 * an immutable object and serializes access for the duration of inspection.
 * No callback request exceeds 32 bytes; no object-sized allocation is made. */
typedef bool (*osg_render_read)(void *context, uint32_t offset, void *destination, size_t bytes);

typedef struct {
    uint32_t object_bytes[OSG_RENDER_OBJECT_KINDS];
    uint32_t pages, nodes, variables, bindings, hit_regions, glyphs, timelines, controls;
    uint32_t expression_instructions;
} osg_render_package_limits;

typedef struct {
    uint16_t kind, record_bytes;
    uint32_t offset, count, length;
} osg_render_section;

typedef struct {
    osg_render_object_kind kind;
    uint32_t object_bytes;
    uint16_t section_count;
    osg_render_section sections[OSG_RENDER_MAX_SECTIONS];
} osg_render_object_info;

extern const osg_render_package_limits OSG_RENDER_SUNTON_LIMITS;
extern const osg_render_package_limits OSG_RENDER_WAVESHARE_LIMITS;

/* Returns zero for unsupported object/section IDs. */
uint16_t osg_render_section_record_bytes(osg_render_object_kind kind, uint16_t section);

/* Structural inspection only. A successful result is not scene admission:
 * hashes, trusted publication/semantics, record fields/references, operation
 * support and total package/runtime resource claims still require validation.
 * expected_bytes comes from the bounded content-store object descriptor.
 * On failure, output is zeroed; no partially validated directory is exposed. */
osg_render_package_status osg_render_object_inspect(
    osg_render_read read, void *context, uint32_t expected_bytes,
    osg_render_object_kind expected_kind, const osg_render_package_limits *limits,
    osg_render_object_info *output);

#endif
