#include "settings.h"
#include <math.h>
#include <string.h>
bool osg_secret(const char *s) {
    if (!s || strlen(s) != 43) return false;
    for (size_t i = 0; i < 43; i++) if (!((s[i] >= 'a' && s[i] <= 'z') || (s[i] >= 'A' && s[i] <= 'Z') || (s[i] >= '0' && s[i] <= '9') || s[i] == '_' || s[i] == '-')) return false;
    /* 32-byte URL-safe base64 without padding has two zero low bits. */
    const char *alphabet="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    const char *last=strchr(alphabet,s[42]); return last && ((last-alphabet)%4)==0;
}
bool osg_settings_parse(const cJSON *v, osg_settings *s) {
    if (!v || !s || !cJSON_IsObject(v)) return false;
    const cJSON *pair = cJSON_GetObjectItemCaseSensitive(v,"pairing"), *clock = cJSON_GetObjectItemCaseSensitive(v,"unixMs");
    const cJSON *expires=cJSON_GetObjectItemCaseSensitive(pair,"expiresAtMs"), *version=cJSON_GetObjectItemCaseSensitive(pair,"version");
    const char *ssid=osg_string(v,"ssid"), *password=osg_string(v,"password"), *name=osg_string(v,"name"), *instance=osg_string(v,"instanceId");
    char host[254]; int port;
    if (!ssid || !*ssid || strlen(ssid)>32 || !password || strlen(password)>64 ||
        !name || !*name || strlen(name)>80 || !cJSON_IsNumber(clock) || clock->valuedouble<1735689600000.0 || clock->valuedouble>4102444800000.0 || floor(clock->valuedouble)!=clock->valuedouble ||
        !cJSON_IsNumber(expires) || expires->valuedouble<0 || expires->valuedouble>9007199254740991.0 || floor(expires->valuedouble)!=expires->valuedouble ||
        !cJSON_IsNumber(version) || version->valuedouble!=1 || !osg_endpoint(osg_string(pair,"endpoint"),host,&port) ||
        !osg_stable_id(osg_string(pair,"hostId")) || !osg_hash_id(osg_string(pair,"caSpkiSha256")) || !osg_secret(osg_string(pair,"secret")) ||
        (instance && *instance && !osg_stable_id(instance))) return false;
    const cJSON *persist=cJSON_GetObjectItemCaseSensitive(v,"persist");
    if(persist&&!cJSON_IsBool(persist)) return false;
    memset(s,0,sizeof(*s));s->schema=1;s->unix_ms=(uint64_t)clock->valuedouble;s->expires_ms=(uint64_t)expires->valuedouble;s->persist=cJSON_IsTrue(persist);
    return osg_copy(s->ssid,sizeof(s->ssid),ssid)&&osg_copy(s->password,sizeof(s->password),password)&&osg_copy(s->name,sizeof(s->name),name)&&
        osg_copy(s->endpoint,sizeof(s->endpoint),osg_string(pair,"endpoint"))&&osg_copy(s->ca,sizeof(s->ca),osg_string(pair,"caCertificate"))&&s->ca[0]&&
        osg_copy(s->host,sizeof(s->host),osg_string(pair,"hostId"))&&osg_copy(s->pin,sizeof(s->pin),osg_string(pair,"caSpkiSha256"))&&
        osg_copy(s->secret,sizeof(s->secret),osg_string(pair,"secret"))&&osg_copy(s->instance,sizeof(s->instance),instance?instance:"");
}
