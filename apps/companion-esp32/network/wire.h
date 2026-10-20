#ifndef OSG_WIRE_H
#define OSG_WIRE_H
#include "companion.h"
#include "cJSON.h"
#define OSG_FRAME_BYTES 16384
#define OSG_REPLY_BYTES 32768
#define OSG_DOCUMENT_BYTES 16384
#define OSG_JSON_HEAP_BYTES (96 * 1024)
#define OSG_IMAGE_TEXT_BYTES 8192

typedef struct {
    bool (*sha256)(const void *, size_t, char out[72]);
    bool (*base64)(const char *, unsigned char *, size_t, size_t *);
} osg_crypto;
typedef struct {
    osg_panel panel;
    osg_page pages[OSG_MAX_PAGES];
    osg_widget widgets[OSG_MAX_WIDGETS];
    const char *image_hashes[OSG_MAX_WIDGETS];
    char strings[OSG_IMAGE_TEXT_BYTES];
    size_t used;
} osg_image;
typedef struct {
    char *bytes;
    size_t capacity, size, expected, index;
    char id[65], hash[72];
    bool active;
} osg_transfer;
typedef struct {
    char client[65], session[65], generation[21], publication[72], instance[65];
    uint64_t heartbeat, feedback_sequence;
    bool feedback_seen, feedback_envelope;
    uint64_t binding_generation;
    uint8_t feedback_bindings[OSG_MAX_WIDGETS], binding_count;
} osg_live_session;
/* JSON hooks are global: initialize once; only the network task may use cJSON. */
void osg_json_init(void);
size_t osg_json_allocated(void);
cJSON *osg_json_parse(const char *, size_t);
const char *osg_string(const cJSON *, const char *);
bool osg_copy(char *, size_t, const char *);
bool osg_counter(const char *, uint64_t *);
bool osg_stable_id(const char *);
bool osg_hash_id(const char *);
bool osg_endpoint(const char *, char host[254], int *port);
/* Returns -1 on error, 0 awaiting transfer chunks, 1 with caller-owned JSON. */
int osg_reply_feed(osg_transfer *, const char *, size_t, osg_crypto, cJSON **);
bool osg_decode_publication(const cJSON *, const char *expected_hash, osg_crypto, osg_image *, uint16_t width, uint16_t height);
bool osg_open_identity(const cJSON *, const char *host, const char *companion, const char *instance, const char *publication, osg_live_session *);
bool osg_ack(const cJSON *, osg_live_session *, const char *sequence, osg_runtime *, uint64_t now);
#endif
