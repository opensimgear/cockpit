#include "serial_json.h"
#include "settings.h"
#include "mbedtls/platform_util.h"

static char line[OSG_PROVISION_BYTES + 1];
static size_t used;
static bool overflow;

osg_serial_record osg_serial_json_byte(int byte, cJSON **record) {
    *record = NULL;
    if (byte != '\n' && byte != '\r') {
        if (!overflow) {
            if (used < OSG_PROVISION_BYTES) line[used++] = (char)byte;
            else overflow = true;
        }
        return OSG_SERIAL_INCOMPLETE;
    }
    osg_serial_record result = OSG_SERIAL_INCOMPLETE;
    if (overflow) result = OSG_SERIAL_TOO_LARGE;
    else if (used) {
        *record = osg_json_parse(line, used);
        result = *record ? OSG_SERIAL_JSON : OSG_SERIAL_INVALID_JSON;
    }
    mbedtls_platform_zeroize(line, sizeof(line));
    used = 0;
    overflow = false;
    return result;
}
