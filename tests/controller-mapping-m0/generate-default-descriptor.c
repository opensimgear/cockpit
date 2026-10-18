#include <stdio.h>
#define DEV_GEN 1
#define DEV_ADDITIONAL 0
#define DEV_FFB 0
#define TLID 1
#define VJOY_AXIS_MAX_VALUE 0x7FFF
unsigned char descriptor[] = {
#include "upstream/vjoy-hidReportDescSingle.h"
};
int main(void) {
    for (unsigned i=0; i<sizeof(descriptor); ++i) printf("%02X",descriptor[i]);
    return 0;
}
