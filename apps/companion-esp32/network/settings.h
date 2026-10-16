#ifndef OSG_SETTINGS_H
#define OSG_SETTINGS_H
#include "wire.h"
#define OSG_PROVISION_BYTES 8192
typedef struct {
    unsigned schema;
    char ssid[33], password[65], endpoint[321], ca[2049], pin[72], host[65];
    char companion[65], name[81], secret[65], token[65], instance[65];
    uint64_t unix_ms, expires_ms;
    bool persist, direct;
} osg_settings;
bool osg_settings_parse(const cJSON *, osg_settings *);
bool osg_secret(const char *);
#endif
