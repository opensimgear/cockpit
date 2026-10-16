#ifndef OSG_RENDER_EXPRESSIONS_H
#define OSG_RENDER_EXPRESSIONS_H

#include "package.h"

#define OSG_EXPRESSION_Q 65536
#define OSG_EXPRESSION_MAX_INSTRUCTIONS 1024u
#define OSG_EXPRESSION_MAX_STACK 8u
#define OSG_EXPRESSION_MAX_LOOKUP_POINTS 256u

typedef enum {
    OSG_EXPRESSION_OK = 0,
    OSG_EXPRESSION_ARGUMENT,
    OSG_EXPRESSION_READ,
    OSG_EXPRESSION_MALFORMED,
    OSG_EXPRESSION_UNAVAILABLE,
    OSG_EXPRESSION_OVERFLOW,
    OSG_EXPRESSION_DIVISION_BY_ZERO,
    OSG_EXPRESSION_INVALID_BOOLEAN,
    OSG_EXPRESSION_INVALID_VALUE
} osg_expression_status;

typedef enum {
    OSG_EXPRESSION_BOOLEAN = 1,
    OSG_EXPRESSION_NUMBER = 2,
    OSG_EXPRESSION_TEXT = 3
} osg_expression_type;

/* Immutable snapshot owned by the caller. Numbers are actual finite source
 * values, converted to signed Q16.16 on read. Boolean values must be 0 or 1.
 * Text and unavailable samples cannot be read by a scalar expression. The
 * caller applies the admitted semantic range/fallback/freshness policy. */
typedef struct {
    osg_expression_type type;
    bool available;
    double value;
} osg_expression_variable;

typedef struct {
    osg_render_read read;
    void *context;
    uint32_t object_bytes;
    uint32_t instruction_offset;
    uint16_t instruction_count;
    uint32_t lookup_offset;
    uint32_t lookup_count;
} osg_expression_program;

/* Truncate toward zero like the TS toFixed reference. Failure clears output. */
osg_expression_status osg_expression_from_number(double value, int32_t *output);

/* Interprets ABI 1 little-endian 16-byte records, with at most eight stack
 * scalars and no heap allocation. Each lookup validates at most 256 sorted
 * 8-byte points; no callback read exceeds 16 bytes. Caller must provide an
 * immutable, structurally admitted object and stable variable snapshot.
 * Runtime failure returns an invalid result (zeroed output), never an event.
 * This does not authorize content or replace scene/property-bound checks. */
osg_expression_status osg_expression_evaluate(const osg_expression_program *program,
    const osg_expression_variable *variables, uint16_t variable_count, int32_t *output);

#endif