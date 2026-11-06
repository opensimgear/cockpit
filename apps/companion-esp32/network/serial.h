#ifndef OSG_SERIAL_H
#define OSG_SERIAL_H
#include <stdbool.h>
bool osg_serial_start(void);
/* Consume a retained receive notification on the console owner. */
bool osg_serial_pending(void);
bool osg_serial_has_wake(void);
#endif
