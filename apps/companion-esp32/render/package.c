#include "package.h"
#include <string.h>

/* ABI 1 section IDs/record bytes: packages/companion-renderer/src/abi.ts.
 * The native runner compares these tables with the host constants. */
static const uint16_t record_bytes[OSG_RENDER_OBJECT_KINDS][OSG_RENDER_MAX_SECTIONS] = {
    {0, 32, 32, 24, 1},
    {0, 16, 64, 24, 16, 16, 24, 8, 24, 4, 1, 12, 2, 8},
    {0, 24, 1},
    {0, 16, 24, 1}
};
static const char magic[OSG_RENDER_OBJECT_KINDS][4] = {"OSGC", "OSGS", "OSGI", "OSGF"};
static const uint16_t section_counts[OSG_RENDER_OBJECT_KINDS] = {4, 13, 2, 3};

const osg_render_package_limits OSG_RENDER_SUNTON_LIMITS = {
    .object_bytes = {8u * 1024u, 16u * 1024u, 768u * 1024u, 768u * 1024u},
    .pages = 4, .nodes = 128, .variables = 32, .bindings = 128,
    .hit_regions = 32, .glyphs = 256, .timelines = 4, .controls = 32,
    .expression_instructions = 512
};
const osg_render_package_limits OSG_RENDER_WAVESHARE_LIMITS = {
    .object_bytes = {16u * 1024u, 48u * 1024u, 2560u * 1024u, 2560u * 1024u},
    .pages = 4, .nodes = 256, .variables = 64, .bindings = 256,
    .hit_regions = 32, .glyphs = 512, .timelines = 8, .controls = 32,
    .expression_instructions = 1024
};

static uint16_t u16(const uint8_t *bytes) {
    return (uint16_t)((uint16_t)bytes[0] | (uint16_t)bytes[1] << 8);
}
static uint32_t u32(const uint8_t *bytes) {
    return (uint32_t)bytes[0] | (uint32_t)bytes[1] << 8 |
        (uint32_t)bytes[2] << 16 | (uint32_t)bytes[3] << 24;
}
uint16_t osg_render_section_record_bytes(osg_render_object_kind kind, uint16_t section) {
    if ((unsigned)kind >= OSG_RENDER_OBJECT_KINDS || section >= OSG_RENDER_MAX_SECTIONS) return 0;
    return record_bytes[kind][section];
}
static uint32_t section_limit(osg_render_object_kind kind, uint16_t section,
                              const osg_render_package_limits *limits) {
    if (kind == OSG_RENDER_SEMANTICS) {
        switch (section) {
        case 1: return limits->controls;
        case 2: return limits->variables;
        case 3: return limits->hit_regions;
        case 4: return limits->object_bytes[kind];
        }
    } else if (kind == OSG_RENDER_SCENE) {
        switch (section) {
        case 1: return limits->pages;
        case 2: return limits->nodes;
        case 3: return limits->variables;
        case 4: return limits->bindings;
        case 5: return limits->expression_instructions;
        case 6: return limits->timelines;
        case 8: return limits->hit_regions;
        case 10: return limits->object_bytes[kind];
        }
    } else if (kind == OSG_RENDER_FONTS) {
        if (section == 2) return limits->glyphs;
        if (section == 3) return limits->object_bytes[kind];
    } else if (section == 2) return limits->object_bytes[kind];
    return UINT16_MAX; /* Other tables use u16 record references in ABI 1. */
}

osg_render_package_status osg_render_object_inspect(
    osg_render_read read, void *context, uint32_t expected_bytes,
    osg_render_object_kind expected_kind, const osg_render_package_limits *limits,
    osg_render_object_info *output) {
    if (output) memset(output, 0, sizeof(*output));
    if (!read || !limits || !output || (unsigned)expected_kind >= OSG_RENDER_OBJECT_KINDS)
        return OSG_RENDER_PACKAGE_ARGUMENT;
    if (expected_bytes < OSG_RENDER_OBJECT_HEADER_BYTES) return OSG_RENDER_PACKAGE_SIZE;
    if (expected_bytes > limits->object_bytes[expected_kind]) return OSG_RENDER_PACKAGE_BUDGET;
    uint8_t header[OSG_RENDER_OBJECT_HEADER_BYTES];
    if (!read(context, 0, header, sizeof(header))) return OSG_RENDER_PACKAGE_READ;
    if (memcmp(header, magic[expected_kind], 4) || u16(header + 4) != 1 ||
        u16(header + 6) != OSG_RENDER_OBJECT_HEADER_BYTES || u16(header + 18))
        return OSG_RENDER_PACKAGE_HEADER;
    for (unsigned i = 20; i < sizeof(header); i++)
        if (header[i]) return OSG_RENDER_PACKAGE_HEADER;
    if (u32(header + 8) != expected_bytes) return OSG_RENDER_PACKAGE_SIZE;
    const uint32_t directory = u32(header + 12);
    const uint16_t entries = u16(header + 16);
    if (directory != OSG_RENDER_OBJECT_HEADER_BYTES || !entries || entries > section_counts[expected_kind] ||
        entries > (expected_bytes - directory) / OSG_RENDER_DIRECTORY_RECORD_BYTES)
        return OSG_RENDER_PACKAGE_DIRECTORY;
    osg_render_object_info result = {.kind = expected_kind, .object_bytes = expected_bytes,
                                    .section_count = entries};
    uint32_t cursor = directory + (uint32_t)entries * OSG_RENDER_DIRECTORY_RECORD_BYTES;
    uint16_t previous = 0;
    for (unsigned i = 0; i < entries; i++) {
        uint8_t entry[OSG_RENDER_DIRECTORY_RECORD_BYTES];
        if (!read(context, directory + i * OSG_RENDER_DIRECTORY_RECORD_BYTES, entry, sizeof(entry)))
            return OSG_RENDER_PACKAGE_READ;
        osg_render_section *section = &result.sections[i];
        *section = (osg_render_section){.kind = u16(entry), .record_bytes = u16(entry + 2),
            .offset = u32(entry + 4), .count = u32(entry + 8), .length = u32(entry + 12)};
        if (!section->kind || section->kind >= OSG_RENDER_MAX_SECTIONS ||
            !record_bytes[expected_kind][section->kind] || section->kind <= previous ||
            section->record_bytes != record_bytes[expected_kind][section->kind] || !section->count ||
            section->count > UINT32_MAX / section->record_bytes ||
            section->length != section->count * section->record_bytes)
            return OSG_RENDER_PACKAGE_SECTION;
        previous = section->kind;
        if (section->count > section_limit(expected_kind, section->kind, limits))
            return OSG_RENDER_PACKAGE_BUDGET;
        const uint32_t padding = (4 - cursor % 4) % 4;
        if (padding > expected_bytes - cursor) return OSG_RENDER_PACKAGE_SIZE;
        if (section->offset % 4 || section->offset != cursor + padding)
            return OSG_RENDER_PACKAGE_OVERLAP;
        if (section->offset > expected_bytes || section->length > expected_bytes - section->offset)
            return OSG_RENDER_PACKAGE_SIZE;
        if (padding) {
            uint8_t bytes[3];
            if (!read(context, cursor, bytes, padding)) return OSG_RENDER_PACKAGE_READ;
            for (uint32_t p = 0; p < padding; p++)
                if (bytes[p]) return OSG_RENDER_PACKAGE_SECTION;
        }
        cursor = section->offset + section->length;
    }
    if (cursor != expected_bytes) return OSG_RENDER_PACKAGE_SIZE;
    *output = result;
    return OSG_RENDER_PACKAGE_OK;
}
