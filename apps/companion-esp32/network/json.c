#include "wire.h"
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <ctype.h>
#include <stdio.h>

typedef union { max_align_t alignment; size_t bytes; } allocation;
static size_t allocated;
static void *bounded_malloc(size_t n) {
    if (n > OSG_JSON_HEAP_BYTES - sizeof(allocation) || allocated > OSG_JSON_HEAP_BYTES - n - sizeof(allocation)) return NULL;
    allocation *a = malloc(sizeof(*a) + n); if (!a) return NULL;
    a->bytes = n; allocated += n + sizeof(*a); return a + 1;
}
static void bounded_free(void *p) { if (p) { allocation *a = (allocation *)p - 1; allocated -= a->bytes + sizeof(*a); volatile unsigned char *s = p; for (size_t i = 0; i < a->bytes; i++) s[i] = 0; free(a); } }
void osg_json_init(void) { cJSON_Hooks hooks = {bounded_malloc, bounded_free}; cJSON_InitHooks(&hooks); }
size_t osg_json_allocated(void) { return allocated; }
static bool safe_tree(const cJSON *v, int depth, size_t *nodes) {
    if (!v || depth > 24 || ++*nodes > 2048 || (cJSON_IsNumber(v) && !isfinite(v->valuedouble))) return false;
    for (const cJSON *a = v->child; a; a = a->next) {
        if (cJSON_IsObject(v)) for (const cJSON *b = a->next; b; b = b->next)
            if (!a->string || !b->string || !strcmp(a->string, b->string)) return false;
        if (!safe_tree(a, depth + 1, nodes)) return false;
    }
    return true;
}
cJSON *osg_json_parse(const char *s, size_t n) {
    if (!s || !n || n > OSG_REPLY_BYTES || memchr(s, 0, n)) return NULL;
    /* cJSON strings cannot represent embedded NUL. Reject its escaped form too. */
    for (size_t i = 0; i + 5 < n; i++) if (s[i] == '\\' && !memcmp(s + i, "\\u0000", 6)) return NULL;
    const char *end = NULL;
    cJSON *v = cJSON_ParseWithLengthOpts(s, n, &end, false);
    if (!v) return NULL;
    while ((size_t)(end - s) < n && isspace((unsigned char)*end)) end++;
    size_t nodes = 0;
    if ((size_t)(end - s) != n || !safe_tree(v, 0, &nodes)) { cJSON_Delete(v); return NULL; }
    return v;
}
const char *osg_string(const cJSON *v, const char *key) {
    const cJSON *s = cJSON_GetObjectItemCaseSensitive(v, key);
    return cJSON_IsString(s) ? s->valuestring : NULL;
}
bool osg_copy(char *out, size_t capacity, const char *s) {
    if (!s || strlen(s) >= capacity) return false;
    memcpy(out, s, strlen(s) + 1); return true;
}
bool osg_counter(const char *s, uint64_t *out) {
    if (!s || !*s || (s[0] == '0' && s[1])) return false;
    uint64_t value = 0;
    for (size_t i = 0; s[i]; i++) {
        if (s[i] < '0' || s[i] > '9' || value > (UINT64_MAX - (unsigned)(s[i] - '0')) / 10) return false;
        value = value * 10 + (unsigned)(s[i] - '0');
    }
    *out = value; return true;
}
bool osg_stable_id(const char *s) {
    if (!s || !*s || strlen(s) > 64 || !isalnum((unsigned char)*s)) return false;
    for (; *s; s++) if (!((*s >= 'a' && *s <= 'z') || (*s >= 'A' && *s <= 'Z') || (*s >= '0' && *s <= '9') || *s == '_' || *s == '-')) return false;
    return true;
}
bool osg_hash_id(const char *s) {
    if (!s || strlen(s) != 71 || memcmp(s, "sha256:", 7)) return false;
    for (size_t i = 7; i < 71; i++) if (!((s[i] >= '0' && s[i] <= '9') || (s[i] >= 'a' && s[i] <= 'f'))) return false;
    return true;
}
bool osg_endpoint(const char *url, char host[254], int *port) {
    if (!url || strncmp(url, "wss://", 6)) return false;
    const char *start = url + 6, *colon = strchr(start, ':');
    if (!colon || colon == start || colon - start > 253) return false;
    /* First release deliberately supports explicit DNS/IPv4 endpoints only. */
    for (const char *p = start; p < colon; p++) if (!(isalnum((unsigned char)*p) || *p == '.' || *p == '-')) return false;
    const char *slash = strchr(colon, '/');
    if (!slash || strcmp(slash, "/companion") || slash - colon < 2 || slash - colon > 6) return false;
    int value = 0;
    for (const char *p = colon + 1; p < slash; p++) { if (*p < '0' || *p > '9') return false; value = value * 10 + (*p - '0'); }
    if (value < 1 || value > 65535) return false;
    memcpy(host, start, (size_t)(colon - start)); host[colon - start] = 0; *port = value; return true;
}
int osg_reply_feed(osg_transfer *t, const char *bytes, size_t n, osg_crypto crypto, cJSON **out) {
    *out = NULL;
    if (!t || !crypto.sha256 || !crypto.base64 || n > OSG_FRAME_BYTES) return -1;
    cJSON *v = osg_json_parse(bytes, n); if (!v || !cJSON_IsObject(v)) { cJSON_Delete(v); return -1; }
    const char *type = osg_string(v, "type");
    int result = -1;
    if (type && !strcmp(type, "transfer-start")) {
        cJSON *count = cJSON_GetObjectItemCaseSensitive(v, "bytes");
        const char *id = osg_string(v, "id"), *hash = osg_string(v, "sha256");
        if (!t->active && osg_stable_id(id) && osg_hash_id(hash) && cJSON_IsNumber(count) &&
            count->valuedouble > 0 && count->valuedouble <= t->capacity && count->valuedouble <= OSG_REPLY_BYTES && floor(count->valuedouble) == count->valuedouble) {
            t->expected = (size_t)count->valuedouble; t->size = 0; t->index = 0; t->active = true;
            (void)osg_copy(t->id, sizeof(t->id), id); (void)osg_copy(t->hash, sizeof(t->hash), hash); result = 0;
        }
    } else if (type && !strcmp(type, "transfer-chunk")) {
        cJSON *index = cJSON_GetObjectItemCaseSensitive(v, "index");
        const char *id = osg_string(v, "id"), *encoded = osg_string(v, "base64"); size_t count = 0;
        if (t->active && id && !strcmp(id, t->id) && cJSON_IsNumber(index) && index->valuedouble == t->index &&
            encoded && strlen(encoded) <= 11000 && crypto.base64(encoded, (unsigned char *)t->bytes + t->size, t->expected - t->size, &count) && count > 0 && count <= t->expected - t->size) {
            t->size += count; t->index++; result = 0;
        }
    } else if (type && !strcmp(type, "transfer-end")) {
        const char *id = osg_string(v, "id"); char hash[72];
        if (t->active && id && !strcmp(id, t->id) && t->size == t->expected && crypto.sha256(t->bytes, t->size, hash) && !strcmp(hash, t->hash)) {
            cJSON_Delete(v); v = osg_json_parse(t->bytes, t->size);
            if (cJSON_IsObject(v)) { *out = v; result = 1; v = NULL; }
        }
        t->active = false;
    } else if (!t->active) { *out = v; v = NULL; result = 1; }
    if (result < 0) t->active = false;
    cJSON_Delete(v); return result;
}
