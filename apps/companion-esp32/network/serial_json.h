#ifndef OSG_SERIAL_JSON_H
#define OSG_SERIAL_JSON_H
#include "wire.h"

typedef enum {
    OSG_SERIAL_INCOMPLETE,
    OSG_SERIAL_JSON,
    OSG_SERIAL_INVALID_JSON,
    OSG_SERIAL_TOO_LARGE
} osg_serial_record;
/* All console consumers share one decoder. A USB record may start during idle
 * command polling and finish while waiting for a protocol reply, or vice versa.
 * The returned JSON is caller-owned; buffered bytes are cleared on completion. */
osg_serial_record osg_serial_json_byte(int byte, cJSON **record);
#endif
